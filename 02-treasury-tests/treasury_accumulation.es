{
  // ================================================================
  // TREASURY ACCUMULATION CONTRACT v1.0
  // ================================================================
  // Receives 10% block reward from emission contract.
  // Simple guard — no multisig needed since funds can only go to
  // governance or stay in accumulation (no theft vector).
  //
  // Paths:
  //   1. Consolidation — merge multiple accum boxes into one
  //   2. Transfer      — spend to the governance contract
  //
  // The governance script hash is stored as a compile-time constant.
  // It is the blake2b256 of the governance contract's propositionBytes.
  // ================================================================

  val governanceScriptHash: Coll[Byte] = fromBase16("ecce6bc576975c9c17246f316caa34917705fc09416a83326c7af666f599180a")

  // ---- PATH 1: CONSOLIDATION ----
  // Output stays at this same script, value preserved or increased
  val isConsolidation: Boolean = {
    OUTPUTS(0).propositionBytes == SELF.propositionBytes &&
    OUTPUTS(0).value >= SELF.value
  }

  // ---- PATH 2: TRANSFER TO GOVERNANCE ----
  // Output goes to the governance contract (hash-locked)
  val isTransferToGovernance: Boolean = {
    blake2b256(OUTPUTS(0).propositionBytes) == governanceScriptHash
  }

  sigmaProp(isConsolidation || isTransferToGovernance)
}
