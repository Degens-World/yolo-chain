{
  // ================================================================
  // LP FUND GOVERNANCE CONTRACT v1.1
  // ================================================================
  // Governs the 5% LP/market-making allocation from the emission contract.
  // Narrow mandate: funds can ONLY flow to whitelisted destination
  // contract hashes (DEX LP pools, bridge liquidity, market-making).
  // Requires 2-of-3 multisig for all operations.
  //
  // Changes from v1.0 (audit fixes):
  //   - [Audit] R6 (migration height) now read at top level and preserved
  //            across all non-migration paths. Prevents timelock bypass via
  //            consolidation/spend/whitelist-update resetting R6.
  //   - [Audit] R6 MUST be initialized to 0L at deployment (eager ValDef
  //            evaluation of SELF.R6[Long].get would crash if absent).
  //
  // Box layout:
  //   SELF.tokens(0) = singleton LP fund NFT (amount == 1)
  //
  // Register layout:
  //   R4: Coll[Coll[Byte]] -- whitelist of allowed destination script hashes
  //   R5: Long             -- state flag: 0 = normal, 1 = frozen, 2 = migration approved
  //   R6: Long             -- migration approval height (0 = no migration pending)
  //
  // DEPLOYMENT: box MUST be created with R4=whitelist, R5=0L, R6=0L
  //
  // Paths:
  //   1. Spend              -- send funds to whitelisted destinations only
  //   2. Whitelist Update   -- add new destination script hashes (no spending)
  //   3. Consolidation      -- merge accumulation boxes in
  //   4. Freeze             -- emergency pause (sets R5=1, permanent)
  //   5. Migration Approval -- approve migration to new contract (sets R5=2, R6=HEIGHT)
  //   6. Migration Execute  -- move funds to new contract after timelock
  // ================================================================

  // ---- Signers (same as treasury) ----
  val signers = Coll(
    PK("9gzkoMXatUr5s7jMBvjR7hzJyPqAyvZtdYxHZ9giJUEyvZ9nJde"),
    PK("9fZM68VSqtjH3HibZngnQ9sgZXudKBt146mkKHWuur3qV4DKDYk"),
    PK("9eYPis3GAjApr8RQKADhk4ZDaNQC7cM46pJi91jzJDa2RoymEzh")
  )
  val threshold = 2

  // ---- Constants ----
  val timelockBlocks: Long = 12960L  // ~72 hours at 20s blocks

  // ---- Singleton NFT identity ----
  val lpNftId: Coll[Byte] = SELF.tokens(0)._1

  // ---- Read registers (all top-level — evaluated eagerly by Scala compiler) ----
  val whitelist: Coll[Coll[Byte]] = SELF.R4[Coll[Coll[Byte]]].get
  val stateFlag: Long = SELF.R5[Long].get
  val migrationHeight: Long = SELF.R6[Long].get  // MUST be initialized at deployment

  val isFrozen: Boolean = stateFlag == 1L

  val out0 = OUTPUTS(0)

  // ---- Shared NFT preservation check ----
  val nftPreserved: Boolean = {
    out0.tokens.size > 0 &&
    out0.tokens(0)._1 == lpNftId &&
    out0.tokens(0)._2 == 1L
  }

  // ================================================================
  // PATH 1: SPEND -- send funds to whitelisted destinations
  // ================================================================
  // Change box at OUTPUTS(0) preserves contract, NFT, whitelist, flags.
  // All subsequent outputs must go to whitelisted script hashes.
  val isSpend: Boolean = {
    val notFrozen: Boolean = !isFrozen

    // Change box preserves contract and all registers
    val selfPreserved: Boolean = out0.propositionBytes == SELF.propositionBytes
    val whitelistPreserved: Boolean = out0.R4[Coll[Coll[Byte]]].get == whitelist
    val flagPreserved: Boolean = out0.R5[Long].get == stateFlag
    val migHeightPreserved: Boolean = out0.R6[Long].get == migrationHeight

    // All non-change outputs must go to whitelisted destinations
    val spendOutputs: Coll[Box] = OUTPUTS.slice(1, OUTPUTS.size)
    val allWhitelisted: Boolean = spendOutputs.forall { (o: Box) =>
      val h: Coll[Byte] = blake2b256(o.propositionBytes)
      whitelist.exists { (allowed: Coll[Byte]) => allowed == h }
    }

    notFrozen && selfPreserved && whitelistPreserved && flagPreserved &&
    migHeightPreserved && allWhitelisted && nftPreserved
  }

  // ================================================================
  // PATH 2: WHITELIST UPDATE -- add new destination script hashes
  // ================================================================
  // No spending allowed during whitelist update.
  // Can only add entries, not remove existing ones.
  val isWhitelistUpdate: Boolean = {
    val notFrozen: Boolean = !isFrozen
    val selfPreserved: Boolean = out0.propositionBytes == SELF.propositionBytes
    val valuePreserved: Boolean = out0.value >= SELF.value
    val flagPreserved: Boolean = out0.R5[Long].get == stateFlag
    val migHeightPreserved: Boolean = out0.R6[Long].get == migrationHeight

    val newWhitelist: Coll[Coll[Byte]] = out0.R4[Coll[Coll[Byte]]].get

    // Must add at least one entry
    val sizeGrown: Boolean = newWhitelist.size > whitelist.size

    // All existing entries must be preserved (superset check)
    val oldPreserved: Boolean = whitelist.forall { (old: Coll[Byte]) =>
      newWhitelist.exists { (entry: Coll[Byte]) => entry == old }
    }

    notFrozen && selfPreserved && valuePreserved && flagPreserved &&
    migHeightPreserved && sizeGrown && oldPreserved && nftPreserved
  }

  // ================================================================
  // PATH 3: CONSOLIDATION -- merge accumulation boxes
  // ================================================================
  val isConsolidation: Boolean = {
    val notFrozen: Boolean = !isFrozen
    val selfPreserved: Boolean = out0.propositionBytes == SELF.propositionBytes
    val valueGrown: Boolean = out0.value >= SELF.value
    val whitelistPreserved: Boolean = out0.R4[Coll[Coll[Byte]]].get == whitelist
    val flagPreserved: Boolean = out0.R5[Long].get == stateFlag
    val migHeightPreserved: Boolean = out0.R6[Long].get == migrationHeight

    notFrozen && selfPreserved && valueGrown &&
    whitelistPreserved && flagPreserved && migHeightPreserved && nftPreserved
  }

  // ================================================================
  // PATH 4: FREEZE -- emergency pause (permanent, no unfreeze path)
  // ================================================================
  val isFreeze: Boolean = {
    val notAlreadyFrozen: Boolean = !isFrozen
    val selfPreserved: Boolean = out0.propositionBytes == SELF.propositionBytes
    val valuePreserved: Boolean = out0.value >= SELF.value
    val whitelistPreserved: Boolean = out0.R4[Coll[Coll[Byte]]].get == whitelist
    val nowFrozen: Boolean = out0.R5[Long].get == 1L

    notAlreadyFrozen && selfPreserved && valuePreserved &&
    whitelistPreserved && nowFrozen && nftPreserved
  }

  // ================================================================
  // PATH 5: MIGRATION APPROVAL -- approve migration to new contract
  // ================================================================
  // Sets R5=2 (migration flag), R6=HEIGHT (approval height for timelock).
  val isMigrationApproval: Boolean = {
    val notFrozen: Boolean = !isFrozen
    val normalState: Boolean = stateFlag == 0L
    val selfPreserved: Boolean = out0.propositionBytes == SELF.propositionBytes
    val valuePreserved: Boolean = out0.value >= SELF.value
    val whitelistPreserved: Boolean = out0.R4[Coll[Coll[Byte]]].get == whitelist
    val migrationFlagSet: Boolean = out0.R5[Long].get == 2L
    val heightRecorded: Boolean = out0.R6[Long].get == HEIGHT

    notFrozen && normalState && selfPreserved && valuePreserved &&
    whitelistPreserved && migrationFlagSet && heightRecorded && nftPreserved
  }

  // ================================================================
  // PATH 6: MIGRATION EXECUTE -- move funds to new contract
  // ================================================================
  // R5 must be 2 (set by migration approval with timelock).
  // R6 holds approval height. NFT transfers to new contract.
  val isMigrationExecute: Boolean = {
    val migrationApproved: Boolean = stateFlag == 2L
    val timelockPassed: Boolean = HEIGHT > migrationHeight + timelockBlocks

    // NFT must go to OUTPUTS(0) (the new contract)
    val nftTransferred: Boolean = {
      out0.tokens.size > 0 &&
      out0.tokens(0)._1 == lpNftId &&
      out0.tokens(0)._2 == 1L
    }

    migrationApproved && timelockPassed && nftTransferred
  }

  // ================================================================
  // ENTRY GUARD
  // ================================================================
  atLeast(threshold, signers) && sigmaProp(
    isSpend || isWhitelistUpdate || isConsolidation ||
    isFreeze || isMigrationApproval || isMigrationExecute
  )
}
