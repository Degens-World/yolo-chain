{
    // ============================================================
    // BURN RECEIPT — minimal record of cross-chain burn intent
    //
    // Created on the wrapped-supply chain when a user destroys
    // wrapped tokens to begin unlocking on the source chain. The box
    // itself is unspendable in user terms (storage rent will eventually
    // sweep it) — its sole purpose is to exist in the chain's UTXO
    // state as proof, referenced via the foreign chain's Lock
    // contract through an AVL+ membership proof against the block's
    // stateRoot.
    //
    // REGISTERS (set by the burn tx):
    //   R4: Coll[Byte]  destination address on source chain
    //                   (propositionBytes the unlock should pay to)
    //   R5: Long        amount burned (wrapped token units;
    //                   matches the corresponding Lock box's value)
    //   R6: Int         burn height (this chain), for filtering/UX
    //
    // SPEND:
    //   Permanently unspendable. Tokens consumed by the burn tx are
    //   not represented in any output and are thus destroyed at the
    //   protocol level. Storage rent on the host chain reclaims the
    //   minimum-box ERG after the rent period elapses.
    // ============================================================

    sigmaProp(false)
}
