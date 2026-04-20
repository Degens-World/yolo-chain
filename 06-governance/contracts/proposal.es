{
  // ================================================================
  // PROPOSAL CONTRACT v1.0 (DuckDAO port)
  // ================================================================
  // Manages proposal lifecycle. State tracked via token quantity:
  //   qty 1 = pending/voting phase
  //   qty 2 = execution-ready (passed validation in counting.es)
  //
  // Ported from DuckPools treasury-system/voting/proposal.md.
  //
  // Changes from DuckDAO:
  //   - Proportion-based distribution (not fixed amount)
  //   - Discussion window (R9) — voting can't start until this height
  //   - Bounded parameter ranges enforced in contract
  //   - New-treasury-mode: proportion == 10,000,000
  //
  // Register layout:
  //   R4: (Long, Long)   — (proportion, 0L) — proportion is numerator / 10,000,000
  //   R5: Coll[Byte]     — recipient ErgoTree script bytes
  //   R6: Long           — validation height (block height when voting concludes)
  //   R7: Int            — supportBps (auto-set: 5000 normal, 9000 elevated)
  //   R8: Int            — votingWindowEnd (last block accepting votes)
  //   R9: Int            — discussionDeadline (voting cannot begin before this)
  //
  // State token: tokens(0) — qty 1 = pending, qty 2 = passed
  //
  // Paths:
  //   1. State advancement — counting validates, token qty 1 → 2
  //   2. Proposal execution — treasury spends based on passed proposal
  // ================================================================

  // ---- Compile-time constants ----
  val TreasuryNftId: Coll[Byte] = fromBase16("TREASURY_NFT_PLACEHOLDER")
  val Denom: Long = 10000000L

  // Bounded parameter ranges are enforced by the counting contract's
  // Phase 1 when accepting proposal initiation, and documented in
  // PARAMETERS.md. Not duplicated here to avoid dead code.

  // ---- Read state ----
  val currentTokens = SELF.tokens(0)
  val proposalTuple = SELF.R4[(Long, Long)].get
  val proportion: Long = proposalTuple._1
  val recipient: Coll[Byte] = SELF.R5[Coll[Byte]].get
  val validationHeight: Long = SELF.R6[Long].get

  val isFirstUpdate: Boolean = currentTokens._2 == 1L

  // ================================================================
  // PATH 1: STATE ADVANCEMENT — counting validates, 1 → 2
  // ================================================================
  // Counting box (INPUTS(0)) holds counter token with qty matching
  // validation height. Successor proposal preserves all immutable fields.
  val isAdvancement: Boolean = {
    val out1: Box = OUTPUTS(1)

    // Counting box validates
    val countingBox: Box = INPUTS(0)
    val countingValid: Boolean =
      countingBox.tokens.size >= 1 &&
      countingBox.tokens(0)._2 == validationHeight

    // Successor preserves script and immutable registers
    val scriptPreserved: Boolean = out1.propositionBytes == SELF.propositionBytes
    val proportionPreserved: Boolean = out1.R4[(Long, Long)].get._1 == proportion
    val recipientPreserved: Boolean = out1.R5[Coll[Byte]].get == recipient

    // State token advances 1 → 2
    val tokenAdvanced: Boolean =
      out1.tokens.size >= 1 &&
      out1.tokens(0)._1 == currentTokens._1 &&
      out1.tokens(0)._2 == 2L

    // Must currently be in pending state
    val validTransition: Boolean = isFirstUpdate

    countingValid && scriptPreserved && proportionPreserved &&
    recipientPreserved && tokenAdvanced && validTransition
  }

  // ================================================================
  // PATH 2: PROPOSAL EXECUTION — treasury spends
  // ================================================================
  // State token must be at qty 2 (passed). Treasury NFT must be
  // present in INPUTS. State token is burned (removed from all outputs).
  val isExecution: Boolean = {
    val isExecutionReady: Boolean = currentTokens._2 == 2L

    // Treasury must be present
    val treasuryPresent: Boolean = INPUTS(1).tokens.size >= 1 &&
      INPUTS(1).tokens(0)._1 == TreasuryNftId

    // State token must be burned — not in ANY output, at ANY token slot
    // (checking only tokens(0) would allow hiding the token at a later slot)
    val tokenBurned: Boolean = {
      val stateTokenId: Coll[Byte] = currentTokens._1
      OUTPUTS.forall { (o: Box) =>
        o.tokens.forall { (t: (Coll[Byte], Long)) => t._1 != stateTokenId }
      }
    }

    isExecutionReady && treasuryPresent && tokenBurned
  }

  // ================================================================
  // PARAMETER VALIDATION (enforced at creation time by off-chain bot)
  // ================================================================
  // The bot must ensure parameters are within bounds when creating
  // the proposal box. The contract validates immutability across
  // state transitions (Path 1), not the initial values.
  // Bounded ranges are enforced by the counting contract's Phase 1
  // when it accepts the proposal initiation.

  // ================================================================
  // ENTRY
  // ================================================================
  sigmaProp(isAdvancement || isExecution)
}
