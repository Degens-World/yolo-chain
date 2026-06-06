{
    // ============================================================
    // DOUBLE-SPEND PREVENTION — append-only AVL set of consumed ids
    //
    // Holds an AVL+ tree whose keys are the ids of foreign-chain
    // boxes (BurnReceipt ids on the unlock direction, Lock ids on
    // the mint direction) that have already been redeemed via the
    // bridge. Lock and Mint contracts spend this box alongside their
    // own input, append the just-claimed id to the tree, and produce
    // the updated box as OUTPUTS(1) (Lock) or OUTPUTS(2) (Mint).
    //
    // The append-once mechanic *is* the double-spend prevention:
    // `AvlTree.insert` returns `None` if the key already exists, so
    // calling `.get` on the resulting `Option` reverts the spend.
    // No additional contract logic is needed.
    //
    // REGISTERS:
    //   R4: AvlTree     consumed-ids set (insert-only flags)
    //
    // TOKENS:
    //   #0: DSP singleton NFT (preserves identity across spends)
    //
    // CONTEXT VARS:
    //   #0: Coll[Byte]  AVL insert proof for the new id
    //
    // EXPECTED SPEND SHAPE:
    //   The companion Lock/Mint contract guarantees that
    //   OUTPUTS(<dsp-out-idx>).R5[Coll[Byte]] holds the id being
    //   consumed, and that propBytes + tokens + value are preserved.
    //   This contract checks the same on whichever output carries
    //   the same NFT (resilient to Lock-spend OUTPUTS(1) vs Mint-spend
    //   OUTPUTS(2) layouts).
    // ============================================================

    val selfTree = SELF.R4[AvlTree].get
    val selfNft  = SELF.tokens(0)._1

    // Locate the DSP successor output by NFT — Lock spends place it at
    // OUTPUTS(1), Mint spends at OUTPUTS(2). Searching both keeps DSP
    // agnostic to the calling contract's layout.
    val successor =
        if      (OUTPUTS(1).tokens(0)._1 == selfNft) OUTPUTS(1)
        else if (OUTPUTS(2).tokens(0)._1 == selfNft) OUTPUTS(2)
        else SELF   // forces validTransition to fail if neither matches

    val added       = successor.R5[Coll[Byte]].get
    val proof       = getVar[Coll[Byte]](0).get
    val insertOps   = Coll((added, added))
    val expectedTree= selfTree.insert(insertOps, proof).get

    val validTransition =
        successor.value             == SELF.value             &&
        successor.tokens            == SELF.tokens            &&
        successor.propositionBytes  == SELF.propositionBytes  &&
        successor.R4[AvlTree].get.digest == expectedTree.digest

    sigmaProp(validTransition)
}
