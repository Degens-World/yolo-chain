{
    // ============================================================
    // LOCK CONTRACT — symmetric template (v2)
    // Holds native chain assets until burn-proof on foreign chain unlocks.
    //
    // CHANGES vs v1:
    //   - DSP box now wired as INPUTS[1], not dataInputs[1]; it must be
    //     spent + recreated to actually prevent double-spend.
    //   - Removed bogus `targetMatch` between burnBox.R4 and SELF.R4
    //     (different chains, different address spaces).
    //   - Added `unlockPaymentOk` constraining OUTPUTS[0] to pay the
    //     unlocked funds to burnBox.R4 (the user's address on this chain).
    //
    // CONFIG (injected as constants at deploy):
    //   - foreignRelayNftId      : 32 bytes
    //   - nullifierNftId         : 32 bytes
    //   - expectedBurnPropHash   : 32 bytes (blake2b256 of Burn contract propBytes on foreign chain)
    //   - minForeignConfs        : Long (e.g. 30)
    //
    // BOX REGISTERS at lock creation:
    //   R4: Coll[Byte]  destination address (propBytes) on foreign chain
    //                   (used by Mint contract during wrap; NOT used for unlock)
    //
    // CONTEXT VARS for unlock spend:
    //   #0: Box         BurnReceipt box from foreign chain (with bytes + id)
    //   #1: Coll[Byte]  AVL+ proof: burnBox is in foreign state at headerId's stateRoot
    //   #2: Coll[Byte]  serialized foreign-chain Header bytes
    //   #3: Coll[Byte]  AVL proof: headerId is in relay's best-chain tree
    //
    // INPUTS:
    //   #0: SELF (this Lock box)
    //   #1: DSP box (verified by NFT; spent + recreated as OUTPUTS[1])
    //
    // DATA INPUTS:
    //   #0: foreign chain Relay box (verified by NFT)
    //
    // OUTPUTS:
    //   #0: unlocked funds at burnBox.R4 (destination on this chain)
    //   #1: updated DSP box (with burnBox.id appended to consumed set)
    // ============================================================

    val foreignRelayNftId    = fromBase16("0000000000000000000000000000000000000000000000000000000000000001")
    val nullifierNftId       = fromBase16("0000000000000000000000000000000000000000000000000000000000000002")
    val expectedBurnPropHash = fromBase16("0000000000000000000000000000000000000000000000000000000000000003")
    val minForeignConfs      = 30L

    val foreignRelay = CONTEXT.dataInputs(0)
    val nullifierBox = INPUTS(1)

    // ───── Authenticate data inputs via NFT ─────
    val properRelay     = foreignRelay.tokens(0)._1 == foreignRelayNftId
    val properNullifier = nullifierBox.tokens(0)._1 == nullifierNftId

    // ───── Parse foreign header + verify it's in the relay's best chain ─────
    val foreignHdrBytes = getVar[Coll[Byte]](2).get
    val h               = Global.deserializeTo[Header](foreignHdrBytes)
    val bestChain       = foreignRelay.R4[AvlTree].get
    val headerProof     = getVar[Coll[Byte]](3).get
    val hdrEntry        = bestChain.get(h.id, headerProof).get
    val hdrHeight       = byteArrayToLong(hdrEntry)

    // ───── Confirmation depth ─────
    val tipHeight   = foreignRelay.R6[Int].get
    val enoughConfs = (tipHeight.toLong - hdrHeight) >= minForeignConfs

    // ───── Verify BurnReceipt exists in foreign UTXO state ─────
    val foreignState = h.stateRoot
    val burnBox      = getVar[Box](0).get
    val stateProof   = getVar[Coll[Byte]](1).get
    val boxInState   = foreignState.get(burnBox.id, stateProof).get == burnBox.bytes

    // ───── Verify BurnReceipt content ─────
    val burnScriptOk = blake2b256(burnBox.propositionBytes) == expectedBurnPropHash
    val amountMatch  = burnBox.R5[Long].get == SELF.value

    // ───── DSP successor: OUTPUTS(1), R5 == burnBox.id (DSP enforces tree insert) ─────
    val nullOut     = OUTPUTS(1)
    val nullUpdated =
        nullOut.tokens(0)._1            == nullifierNftId  &&
        nullOut.R5[Coll[Byte]].get      == burnBox.id

    // ───── Unlock payment: OUTPUTS(0) goes to burnBox.R4 ─────
    val unlockOut       = OUTPUTS(0)
    val unlockDest      = burnBox.R4[Coll[Byte]].get
    val unlockPaymentOk =
        unlockOut.propositionBytes == unlockDest  &&
        unlockOut.value            >= SELF.value

    sigmaProp(
        properRelay && properNullifier && enoughConfs &&
        boxInState && burnScriptOk && amountMatch &&
        nullUpdated && unlockPaymentOk
    )
}
