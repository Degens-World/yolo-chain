{
    // ============================================================
    // MINT CONTRACT — symmetric template
    // Holds wrapped-token supply on destination chain.
    // Releases wrapped tokens to user upon proof of Lock box on foreign chain.
    //
    // CONFIG (injected as constants at deploy):
    //   - foreignRelayNftId   : 32 bytes
    //   - nullifierNftId      : 32 bytes
    //   - expectedLockPropHash: 32 bytes (blake2b256 of the Lock contract's propBytes on foreign chain)
    //   - minForeignConfs     : Long
    //   - wrappedTokenId      : 32 bytes (the wrapped asset's token id, pre-minted into this box)
    //
    // BOX TOKENS (at deploy & every spend):
    //   #0: mint-contract NFT (singleton, preserved)
    //   #1: wrapped supply (large; decreases as users redeem locks)
    //
    // CONTEXT VARS for mint spend:
    //   #0: Box         foreign Lock box (proves intent + destination + amount)
    //   #1: Coll[Byte]  AVL+ proof that Lock box is in foreign UTXO state
    //   #2: Coll[Byte]  serialized foreign header bytes
    //   #3: Coll[Byte]  AVL proof that header is in relay best-chain
    //   #4: Coll[Byte]  AVL nullifier insert proof
    //
    // DATA INPUTS:
    //   #0: foreign chain Relay box
    //   #1: DoubleSpendPrevention box
    //
    // OUTPUTS:
    //   #0: updated Mint box (wrapped supply reduced by lockAmount)
    //   #1: user payment box (lockAmount of wrappedTokenId at the destination prop)
    //   #2: updated nullifier box
    // ============================================================

    val foreignRelayNftId     = fromBase16("0000000000000000000000000000000000000000000000000000000000000001")
    val nullifierNftId        = fromBase16("0000000000000000000000000000000000000000000000000000000000000002")
    val expectedLockPropHash  = fromBase16("0000000000000000000000000000000000000000000000000000000000000004")
    val minForeignConfs       = 30L
    val wrappedTokenId        = fromBase16("0000000000000000000000000000000000000000000000000000000000000005")

    val foreignRelay = CONTEXT.dataInputs(0)
    val nullifierBox = CONTEXT.dataInputs(1)

    // ───── Authenticate data inputs ─────
    val properRelay     = foreignRelay.tokens(0)._1 == foreignRelayNftId
    val properNullifier = nullifierBox.tokens(0)._1 == nullifierNftId

    // ───── Header + best-chain membership ─────
    val hdrBytes    = getVar[Coll[Byte]](2).get
    val h           = Global.deserializeTo[Header](hdrBytes)
    val bestChain   = foreignRelay.R4[AvlTree].get
    val headerProof = getVar[Coll[Byte]](3).get
    val hdrEntry    = bestChain.get(h.id, headerProof).get
    val hdrHeight   = byteArrayToLong(hdrEntry)

    val tipHeight   = foreignRelay.R6[Int].get
    val enoughConfs = (tipHeight.toLong - hdrHeight) >= minForeignConfs

    // ───── Lock box must exist in foreign state ─────
    val foreignState = h.stateRoot
    val lockBox      = getVar[Box](0).get
    val stateProof   = getVar[Coll[Byte]](1).get
    val boxInState   = foreignState.get(lockBox.id, stateProof).get == lockBox.bytes

    // ───── Lock box content ─────
    val lockScriptOk = blake2b256(lockBox.propositionBytes) == expectedLockPropHash
    val destination  = lockBox.R4[Coll[Byte]].get
    val lockAmount   = lockBox.R5[Long].get

    // ───── Nullifier (prevents double-mint per Lock box) ─────
    val nullTree    = nullifierBox.R4[AvlTree].get
    val nullProof   = getVar[Coll[Byte]](4).get
    val newNullTree = nullTree.insert(Coll((lockBox.id, lockBox.id)), nullProof).get
    val nullOut     = OUTPUTS(2)
    val nullUpdated =
        nullOut.R4[AvlTree].get.digest == newNullTree.digest             &&
        nullOut.propositionBytes       == nullifierBox.propositionBytes &&
        nullOut.tokens                 == nullifierBox.tokens

    // ───── Output #0: Mint box continuation (supply drained by lockAmount) ─────
    val mintOut         = OUTPUTS(0)
    val supplyDrainOk   =
        mintOut.propositionBytes == SELF.propositionBytes  &&
        mintOut.tokens(0)        == SELF.tokens(0)         &&     // NFT preserved
        mintOut.tokens(1)._1     == wrappedTokenId         &&
        (SELF.tokens(1)._2 - mintOut.tokens(1)._2) == lockAmount &&
        mintOut.value            >= SELF.value

    // ───── Output #1: User receives wrapped tokens at destination ─────
    val userOut         = OUTPUTS(1)
    val userPaymentOk   =
        userOut.propositionBytes == destination          &&
        userOut.tokens(0)._1     == wrappedTokenId       &&
        userOut.tokens(0)._2     == lockAmount

    sigmaProp(
        properRelay && properNullifier && enoughConfs &&
        boxInState && lockScriptOk && nullUpdated &&
        supplyDrainOk && userPaymentOk
    )
}
