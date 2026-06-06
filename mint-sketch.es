{
    // ============================================================
    // MINT CONTRACT — symmetric template (v2)
    // Holds wrapped-token supply on destination chain.
    // Releases wrapped tokens to user upon proof of Lock box on foreign chain.
    //
    // CHANGES vs v1:
    //   - DSP box now wired as INPUTS[1], not dataInputs[1]; it must be
    //     spent + recreated to actually prevent double-spend.
    //   - DSP successor is OUTPUTS[2] (mint occupies OUTPUTS[0],
    //     user payment occupies OUTPUTS[1]).
    //
    // CONFIG (injected as constants at deploy):
    //   - foreignRelayNftId   : 32 bytes
    //   - nullifierNftId      : 32 bytes
    //   - expectedLockPropHash: 32 bytes (blake2b256 of Lock contract propBytes on foreign chain)
    //   - minForeignConfs     : Long
    //   - wrappedTokenId      : 32 bytes (pre-minted into this box)
    //
    // BOX TOKENS:
    //   #0: mint-contract NFT (singleton, preserved)
    //   #1: wrapped supply (large; decreases as users redeem locks)
    //
    // CONTEXT VARS for mint spend:
    //   #0: Box         foreign Lock box
    //   #1: Coll[Byte]  AVL+ proof: Lock box is in foreign UTXO state
    //   #2: Coll[Byte]  serialized foreign header bytes
    //   #3: Coll[Byte]  AVL proof: header is in relay best-chain
    //
    // INPUTS:
    //   #0: SELF (this Mint box, drained by lockAmount)
    //   #1: DSP box (verified by NFT; spent + recreated as OUTPUTS[2])
    //
    // DATA INPUTS:
    //   #0: foreign chain Relay box
    //
    // OUTPUTS:
    //   #0: updated Mint box (wrapped supply reduced)
    //   #1: user payment (lockAmount of wrappedTokenId at destination prop)
    //   #2: updated DSP box (with lockBox.id appended)
    // ============================================================

    val foreignRelayNftId     = fromBase16("0000000000000000000000000000000000000000000000000000000000000001")
    val nullifierNftId        = fromBase16("0000000000000000000000000000000000000000000000000000000000000002")
    val expectedLockPropHash  = fromBase16("0000000000000000000000000000000000000000000000000000000000000004")
    val minForeignConfs       = 30L
    val wrappedTokenId        = fromBase16("0000000000000000000000000000000000000000000000000000000000000005")

    val foreignRelay = CONTEXT.dataInputs(0)
    val nullifierBox = INPUTS(1)

    // ───── Authenticate data inputs / DSP ─────
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
    val lockAmount   = lockBox.value

    // ───── DSP successor: OUTPUTS(2), R5 == lockBox.id ─────
    val nullOut     = OUTPUTS(2)
    val nullUpdated =
        nullOut.tokens(0)._1            == nullifierNftId  &&
        nullOut.R5[Coll[Byte]].get      == lockBox.id

    // ───── Mint box continuation: supply drained by exactly lockAmount ─────
    val mintOut       = OUTPUTS(0)
    val supplyDrainOk =
        mintOut.propositionBytes == SELF.propositionBytes  &&
        mintOut.tokens(0)        == SELF.tokens(0)         &&   // NFT preserved
        mintOut.tokens(1)._1     == wrappedTokenId         &&
        (SELF.tokens(1)._2 - mintOut.tokens(1)._2) == lockAmount &&
        mintOut.value            >= SELF.value

    // ───── User payment: OUTPUTS(1) goes to destination ─────
    val userOut       = OUTPUTS(1)
    val userPaymentOk =
        userOut.propositionBytes == destination         &&
        userOut.tokens(0)._1     == wrappedTokenId      &&
        userOut.tokens(0)._2     == lockAmount

    sigmaProp(
        properRelay && properNullifier && enoughConfs &&
        boxInState && lockScriptOk && nullUpdated &&
        supplyDrainOk && userPaymentOk
    )
}
