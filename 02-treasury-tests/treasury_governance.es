{
  // ================================================================
  // TREASURY GOVERNANCE CONTRACT v1.1
  // ================================================================
  // Governs the 10% dev treasury allocation from the emission contract.
  // Requires 2-of-3 multisig for all operations.
  //
  // Changes from v1.0 (audit fixes):
  //   - Added singleton NFT for box identity (all paths preserve it)
  //   - Added migration approval path (sets R8=2 with timelock)
  //   - Approval path now constrains R4 = blake2b256(R6 ++ longToByteArray(R7))
  //
  // Box layout:
  //   SELF.tokens(0) = singleton governance NFT (amount == 1)
  //
  // Register layout:
  //   R4: Coll[Byte] — proposal hash = blake2b256(recipient ++ longToByteArray(amount))
  //   R5: Long       — approval height (0 = no active proposal)
  //   R6: Coll[Byte] — recipient propositionBytes
  //   R7: Long       — approved amount (nanoERG)
  //   R8: Long       — state flag: 0 = normal, 1 = frozen, 2 = migration approved
  //
  // Paths:
  //   1. Approval          — submit a new spending proposal
  //   2. Execution         — spend approved proposal after timelock
  //   3. Cancellation      — clear active proposal (immediate)
  //   4. Consolidation     — merge accumulation boxes in
  //   5. Freeze            — emergency pause (sets R8=1)
  //   6. Migration Approval— approve migration to new contract (sets R8=2)
  //   7. Migration Execute — move funds to new contract after timelock
  // ================================================================

  // ---- Signers ----
  val signers = Coll(
    PK("9gzkoMXatUr5s7jMBvjR7hzJyPqAyvZtdYxHZ9giJUEyvZ9nJde"),
    PK("9fZM68VSqtjH3HibZngnQ9sgZXudKBt146mkKHWuur3qV4DKDYk"),
    PK("9eYPis3GAjApr8RQKADhk4ZDaNQC7cM46pJi91jzJDa2RoymEzh")
  )
  val threshold = 2

  // ---- Constants ----
  val timelockBlocks: Long = 12960L  // ~72 hours at 20s blocks

  // ---- Singleton NFT identity ----
  val governanceNftId: Coll[Byte] = SELF.tokens(0)._1

  // ---- Read registers ----
  val proposalHash: Coll[Byte]  = SELF.R4[Coll[Byte]].get
  val approvalHeight: Long      = SELF.R5[Long].get
  val recipient: Coll[Byte]     = SELF.R6[Coll[Byte]].get
  val amount: Long              = SELF.R7[Long].get
  val stateFlag: Long           = SELF.R8[Long].get

  val isFrozen: Boolean          = stateFlag == 1L
  val hasActiveProposal: Boolean = approvalHeight > 0L

  val out0 = OUTPUTS(0)

  // ---- Shared NFT preservation check ----
  val nftPreserved: Boolean = {
    out0.tokens.size > 0 &&
    out0.tokens(0)._1 == governanceNftId &&
    out0.tokens(0)._2 == 1L
  }

  // ================================================================
  // PATH 1: APPROVAL — submit a new spending proposal
  // ================================================================
  val isApproval: Boolean = {
    val noExistingProposal: Boolean = !hasActiveProposal
    val notFrozen: Boolean          = !isFrozen
    val selfPreserved: Boolean      = out0.propositionBytes == SELF.propositionBytes
    val valuePreserved: Boolean     = out0.value >= SELF.value
    val heightRecorded: Boolean     = out0.R5[Long].get == HEIGHT
    val flagPreserved: Boolean      = out0.R8[Long].get == stateFlag

    // Constrain proposal hash: must equal blake2b256(recipient ++ amount_bytes)
    val hashConsistent: Boolean = {
      out0.R4[Coll[Byte]].get == blake2b256(
        out0.R6[Coll[Byte]].get ++ longToByteArray(out0.R7[Long].get)
      )
    }

    noExistingProposal && notFrozen && selfPreserved && nftPreserved &&
    valuePreserved && heightRecorded && flagPreserved && hashConsistent
  }

  // ================================================================
  // PATH 2: EXECUTION — spend an approved proposal after timelock
  // ================================================================
  val isExecution: Boolean = {
    val notFrozen: Boolean       = !isFrozen
    val timelockPassed: Boolean  = HEIGHT > approvalHeight + timelockBlocks

    // Payment output matches proposal
    val correctPayment: Boolean = {
      OUTPUTS(1).propositionBytes == recipient &&
      OUTPUTS(1).value >= amount
    }

    // Change returned to same governance contract with proposal cleared
    val correctChange: Boolean = {
      out0.propositionBytes == SELF.propositionBytes &&
      out0.value >= SELF.value - amount - 1000000L &&  // allow TX fee
      out0.R5[Long].get == 0L &&                       // clear proposal
      out0.R8[Long].get == 0L                          // reset state flag
    }

    hasActiveProposal && notFrozen && timelockPassed &&
    correctPayment && correctChange && nftPreserved
  }

  // ================================================================
  // PATH 3: CANCELLATION — clear an active proposal (immediate)
  // ================================================================
  val isCancellation: Boolean = {
    val selfPreserved: Boolean   = out0.propositionBytes == SELF.propositionBytes
    val valuePreserved: Boolean  = out0.value >= SELF.value
    val proposalCleared: Boolean = out0.R5[Long].get == 0L
    val flagPreserved: Boolean   = out0.R8[Long].get == stateFlag

    hasActiveProposal && selfPreserved && valuePreserved &&
    proposalCleared && flagPreserved && nftPreserved
  }

  // ================================================================
  // PATH 4: CONSOLIDATION — merge accumulation boxes into governance
  // ================================================================
  val isConsolidation: Boolean = {
    val notFrozen: Boolean       = !isFrozen
    val selfPreserved: Boolean   = out0.propositionBytes == SELF.propositionBytes
    val valueGrown: Boolean      = out0.value >= SELF.value

    // All registers must be preserved exactly
    val regsPreserved: Boolean = {
      out0.R4[Coll[Byte]].get == proposalHash &&
      out0.R5[Long].get == approvalHeight &&
      out0.R6[Coll[Byte]].get == recipient &&
      out0.R7[Long].get == amount &&
      out0.R8[Long].get == stateFlag
    }

    notFrozen && selfPreserved && valueGrown && regsPreserved && nftPreserved
  }

  // ================================================================
  // PATH 5: FREEZE — emergency pause
  // ================================================================
  val isFreeze: Boolean = {
    val notAlreadyFrozen: Boolean = !isFrozen
    val selfPreserved: Boolean    = out0.propositionBytes == SELF.propositionBytes
    val valuePreserved: Boolean   = out0.value >= SELF.value
    val nowFrozen: Boolean        = out0.R8[Long].get == 1L

    // Other registers must be preserved
    val otherRegsPreserved: Boolean = {
      out0.R4[Coll[Byte]].get == proposalHash &&
      out0.R5[Long].get == approvalHeight &&
      out0.R6[Coll[Byte]].get == recipient &&
      out0.R7[Long].get == amount
    }

    notAlreadyFrozen && selfPreserved && valuePreserved &&
    nowFrozen && otherRegsPreserved && nftPreserved
  }

  // ================================================================
  // PATH 6: MIGRATION APPROVAL — approve migration to new contract
  // ================================================================
  val isMigrationApproval: Boolean = {
    val noExistingProposal: Boolean = !hasActiveProposal
    val notFrozen: Boolean          = !isFrozen
    val notMigrating: Boolean       = stateFlag == 0L
    val selfPreserved: Boolean      = out0.propositionBytes == SELF.propositionBytes
    val valuePreserved: Boolean     = out0.value >= SELF.value
    val heightRecorded: Boolean     = out0.R5[Long].get == HEIGHT
    val migrationFlagSet: Boolean   = out0.R8[Long].get == 2L

    noExistingProposal && notFrozen && notMigrating && selfPreserved &&
    valuePreserved && heightRecorded && migrationFlagSet && nftPreserved
  }

  // ================================================================
  // PATH 7: MIGRATION EXECUTE — move funds to new contract
  // ================================================================
  // R8 must be 2 (set by migration approval path with timelock).
  // NFT transfers to the new contract — one-time irreversible.
  val isMigrationExecute: Boolean = {
    val migrationApproved: Boolean = stateFlag == 2L
    val timelockPassed: Boolean    = HEIGHT > approvalHeight + timelockBlocks

    // NFT must go to OUTPUTS(0) (the new contract)
    val nftTransferred: Boolean = {
      out0.tokens.size > 0 &&
      out0.tokens(0)._1 == governanceNftId &&
      out0.tokens(0)._2 == 1L
    }

    migrationApproved && timelockPassed && nftTransferred
  }

  // ================================================================
  // ENTRY GUARD
  // ================================================================
  atLeast(threshold, signers) && sigmaProp(
    isApproval || isExecution || isCancellation ||
    isConsolidation || isFreeze || isMigrationApproval || isMigrationExecute
  )
}
