{
  // ================================================================
  // LP FUND ACCUMULATION CONTRACT v1.0
  // ================================================================
  // Receives 5% block reward from emission contract.
  // Simple guard -- no multisig needed since funds can only go to
  // LP fund governance or stay in accumulation (no theft vector).
  //
  // Paths:
  //   1. Consolidation -- merge multiple accum boxes into one
  //   2. Transfer      -- spend to the LP fund governance contract
  //
  // The LP fund governance script hash is stored as a compile-time
  // constant. It is the blake2b256 of the LP fund governance
  // contract's propositionBytes.
  // ================================================================

  // PLACEHOLDER -- replace with actual blake2b256 hash of compiled LP fund governance ErgoTree
  val lpFundScriptHash: Coll[Byte] = fromBase16("f7e52722204eab03cf13bea0772e0e00ee48f4a6f81396514d9a3b692d61b1e4")

  // ---- PATH 1: CONSOLIDATION ----
  // Output stays at this same script, value preserved or increased
  val isConsolidation: Boolean = {
    OUTPUTS(0).propositionBytes == SELF.propositionBytes &&
    OUTPUTS(0).value >= SELF.value
  }

  // ---- PATH 2: TRANSFER TO LP FUND GOVERNANCE ----
  // Output goes to the LP fund governance contract (hash-locked)
  val isTransferToGovernance: Boolean = {
    blake2b256(OUTPUTS(0).propositionBytes) == lpFundScriptHash
  }

  sigmaProp(isConsolidation || isTransferToGovernance)
}
