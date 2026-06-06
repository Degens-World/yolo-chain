{
    // ============================================================
    // SYMMETRIC AUTOLYKOS2 RELAY — Sigma 6.0 sketch
    // Deploy on either chain (YOLO or Ergo) to verify the other's headers.
    //
    // REGISTERS
    //   R4: AvlTree     best-chain digest      (id -> height bytes)
    //   R5: AvlTree     all-headers digest     (id -> height ++ cumWork bytes)
    //   R6: Int         tip height
    //   R7: Coll[Byte]  tip block id
    //   R8: BigInt      tip cumulative work
    //
    // TOKENS  #0: relay NFT (singleton)
    //
    // CONTEXT VARS
    //   #1: Coll[Byte]  serialized foreign-chain Header bytes
    //   #2: Coll[Byte]  best-chain insert proof (tip-extension path)
    //   #3: Coll[Byte]  parent lookup proof (all-headers tree)
    //   #4: AvlTree     parent's best-chain snapshot (reorg path)
    //   #5: Coll[Byte]  parent's best-chain insert proof (reorg path)
    //   #6: Coll[Byte]  all-headers insert proof
    //   #7: Coll[Byte]  new header's cumulative work as fixed-width bytes
    // ============================================================

    val bountyDrain = 1000000L

    val bestDigest = SELF.R4[AvlTree].get
    val allDigest  = SELF.R5[AvlTree].get
    val tipHeight  = SELF.R6[Int].get
    val tipId      = SELF.R7[Coll[Byte]].get
    val tipWork    = SELF.R8[BigInt].get
    val selfOut    = OUTPUTS(0)

    // ----- PARSE + PoW (the entire delta from BtcRelay.es) -----
    val headerBytes = getVar[Coll[Byte]](1).get
    val h           = Global.deserializeTo[Header](headerBytes)
    val validPow    = h.checkPow
    val work        = Global.decodeNbits(h.nBits)
    val newHt       = tipHeight + 1
    val bestRow     = (h.id, longToByteArray(newHt.toLong))
    val cumWorkBytes= getVar[Coll[Byte]](7).get

    // ----- BEST-CHAIN UPDATE (tip extension) -----
    val tipExtended = if (h.parentId == tipId) {
        val proof    = getVar[Coll[Byte]](2).get
        val newBest  = bestDigest.insert(Coll(bestRow), proof).get
        val outBest  = selfOut.R4[AvlTree].get
        val cumWork  = tipWork + work

        outBest.digest                == newBest.digest                &&
        outBest.enabledOperations     == bestDigest.enabledOperations  &&
        selfOut.R6[Int].get           == newHt                         &&
        selfOut.R7[Coll[Byte]].get    == h.id                          &&
        selfOut.R8[BigInt].get        == cumWork
    } else { true }

    // ----- ALL-HEADERS UPDATE (always runs; handles reorgs) -----
    val allUpdated = {
        val parentProof  = getVar[Coll[Byte]](3).get
        val parentEntry  = allDigest.get(h.parentId, parentProof).get
        val parentHt     = byteArrayToLong(parentEntry.slice(0, 8))
        val parentWork   = byteArrayToBigInt(parentEntry.slice(8, parentEntry.size))
        val cumWork      = parentWork + work
        val newRow       = (h.id, longToByteArray(parentHt + 1) ++ cumWorkBytes)
        val insertProof  = getVar[Coll[Byte]](6).get
        val newAll       = allDigest.insert(Coll(newRow), insertProof).get
        val outAll       = selfOut.R5[AvlTree].get
        val allOk        =
            outAll.digest             == newAll.digest                 &&
            outAll.enabledOperations  == allDigest.enabledOperations   &&
            byteArrayToBigInt(cumWorkBytes) == cumWork

        if (cumWork > tipWork && h.parentId != tipId) {
            val parentBest      = getVar[AvlTree](4).get
            val parentBestProof = getVar[Coll[Byte]](5).get
            val forkedBest      = parentBest.insert(Coll(bestRow), parentBestProof).get
            val outBest         = selfOut.R4[AvlTree].get

            allOk &&
            outBest.digest                == forkedBest.digest             &&
            outBest.enabledOperations     == bestDigest.enabledOperations  &&
            selfOut.R6[Int].get           == (parentHt + 1).toInt          &&
            selfOut.R7[Coll[Byte]].get    == h.id                          &&
            selfOut.R8[BigInt].get        == cumWork
        } else {
            allOk
        }
    }

    // ----- SELF + BOUNTY -----
    val preservation =
        selfOut.value             >= SELF.value - bountyDrain &&
        selfOut.tokens            == SELF.tokens              &&
        selfOut.propositionBytes  == SELF.propositionBytes

    sigmaProp(validPow && tipExtended && allUpdated && preservation)
}
