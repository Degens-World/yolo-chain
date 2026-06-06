{
    // ============================================================
    // LOCK CONTRACT — symmetric template
    // Holds native chain assets until burn-proof on foreign chain unlocks.
    //
    // CONFIG (injected as constants at deploy):
    //   - foreignRelayNftId      : 32 bytes (identifies the foreign-chain Relay box on THIS chain)
    //   - nullifierNftId         : 32 bytes (identifies the DoubleSpendPrevention box)
    //   - expectedBurnPropHash   : 32 bytes (blake2b256 of the Burn contract's propBytes on foreign chain)
    //   - minForeignConfs        : Long (e.g. 30)
    //
    // BOX REGISTERS at lock creation:
    //   R4: Coll[Byte]  destination address (propBytes) on foreign chain
    //   R5: Long        amount locked (cross-check against burn)
    //
    // CONTEXT VARS for unlock spend:
    //   #0: Box         BurnReceipt box from foreign chain (with bytes + id)
    //   #1: Coll[Byte]  AVL+ proof: burnBox is in foreign state at headerId's stateRoot
    //   #2: Coll[Byte]  serialized foreign-chain Header bytes
    //   #3: Coll[Byte]  AVL proof: headerId is in relay's best-chain tree
    //   #4: Coll[Byte]  AVL nullifier insert proof
    //
    // DATA INPUTS:
    //   #0: foreign chain Relay box (verified by NFT)
    //   #1: DoubleSpendPrevention box (verified by NFT)
    // ============================================================

    val foreignRelayNftId    = fromBase16("0000000000000000000000000000000000000000000000000000000000000001")
    val nullifierNftId       = fromBase16("0000000000000000000000000000000000000000000000000000000000000002")
    val expectedBurnPropHash = fromBase16("0000000000000000000000000000000000000000000000000000000000000003")
    val minForeignConfs      = 30L

    val foreignRelay = CONTEXT.dataInputs(0)
    val nullifierBox = CONTEXT.dataInputs(1)

    // ───── Authenticate data inputs via NFT ─────
    val properRelay     = foreignRelay.tokens(0)._1 == foreignRelayNftId
    val properNullifier = nullifierBox.tokens(0)._1 == nullifierNftId

    // ───── Parse foreign header + verify it's in the relay's best chain ─────
    val foreignHdrBytes = getVar[Coll[Byte]](2).get
    val h               = Global.deserializeTo[Header](foreignHdrBytes)
    val bestChain       = foreignRelay.R4[AvlTree].get
    val headerProof     = getVar[Coll[Byte]](3).get
    val hdrEntry        = bestChain.get(h.id, headerProof).get  // throws if not on best chain
    val hdrHeight       = byteArrayToLong(hdrEntry)

    // ───── Confirmation depth ─────
    val tipHeight   = foreignRelay.R6[Int].get
    val enoughConfs = (tipHeight.toLong - hdrHeight) >= minForeignConfs

    // ───── Verify BurnReceipt exists in foreign UTXO state ─────
    val foreignState   = h.stateRoot                            // AvlTree, native to Header type
    val burnBox        = getVar[Box](0).get
    val stateProof     = getVar[Coll[Byte]](1).get
    val boxInState     = foreignState.get(burnBox.id, stateProof).get == burnBox.bytes

    // ───── Verify BurnReceipt content ─────
    val burnScriptOk = blake2b256(burnBox.propositionBytes) == expectedBurnPropHash
    val targetMatch  = burnBox.R4[Coll[Byte]].get == SELF.R4[Coll[Byte]].get
    val amountMatch  = burnBox.R5[Long].get       == SELF.R5[Long].get

    // ───── DoubleSpend nullifier update ─────
    val nullTree    = nullifierBox.R4[AvlTree].get
    val nullProof   = getVar[Coll[Byte]](4).get
    val newNullTree = nullTree.insert(Coll((burnBox.id, burnBox.id)), nullProof).get
    val nullOut     = OUTPUTS(1)
    val nullUpdated =
        nullOut.R4[AvlTree].get.digest == newNullTree.digest         &&
        nullOut.propositionBytes       == nullifierBox.propositionBytes &&
        nullOut.tokens                 == nullifierBox.tokens

    sigmaProp(
        properRelay && properNullifier && enoughConfs &&
        boxInState && burnScriptOk && targetMatch && amountMatch &&
        nullUpdated
    )
}
