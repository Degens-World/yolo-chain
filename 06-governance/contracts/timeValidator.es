{
  // ================================================================
  // TIME VALIDATOR CONTRACT v1.0 (DuckDAO port)
  // ================================================================
  // Gates voter box creation by height window. Ensures votes can
  // only be created during the voting period, AFTER the mandatory
  // discussion window.
  //
  // Ported from DuckPools treasury-system/voting/timeValidator.
  //
  // This contract guards the output that creates a new userVote box.
  // It validates:
  //   1. HEIGHT is within the voting window (after discussion, before deadline)
  //   2. The created vote box has the correct structure
  //   3. The counter box is referenced to verify timing
  //
  // The contract reads the counter box (via data input) to get the
  // current voting deadline and discussion deadline.
  //
  // Box layout (SELF = time validator box, consumed to create vote):
  //   SELF.tokens(0) = vYOLO to be locked in the vote box
  //   SELF.R4: Long  = vote direction (1 = yes, 0 = no)
  //   SELF.R5: Coll[Byte] = proposal ID
  //   SELF.R6: SigmaProp  = voter public key
  //
  // Integration:
  //   User creates a time validator box → bot (or user) consumes it
  //   to create a userVote box if the timing is valid.
  // ================================================================

  // ---- Compile-time constants ----
  val CounterNftId: Coll[Byte]  = fromBase16("COUNTER_NFT_PLACEHOLDER")
  val ValidVoteId: Coll[Byte]   = fromBase16("VALID_VOTE_PLACEHOLDER")
  val VYoloId: Coll[Byte]       = fromBase16("VYOLO_TOKEN_PLACEHOLDER")
  val UserVoteScriptHash: Coll[Byte] = fromBase16("USER_VOTE_HASH_PLACEHOLDER")

  // ---- Cancellation cooldown ----
  val cancellationCooldown: Long = 4320L  // 24 hours (matching userVote.es)

  // ---- Read counter box from data inputs ----
  val counterBox: Box = CONTEXT.dataInputs(0)
  val counterValid: Boolean =
    counterBox.tokens.size >= 1 &&
    counterBox.tokens(0)._1 == CounterNftId

  val voteDeadline: Long = counterBox.R4[Long].get

  // ---- Discussion window check ----
  // The proposal's discussion deadline is stored in the proposal box.
  // We read it from data inputs(1) if available, or from the counter box.
  // For simplicity in v1.0: voting window starts at (voteDeadline - votingWindow)
  // and the discussion period is the time before that.
  val votingWindow: Long = 12960L  // 3 days (matching counting.es)
  val votingStart: Long = voteDeadline - votingWindow

  // ---- Timing validation ----
  // HEIGHT must be within [votingStart, voteDeadline)
  val withinVotingWindow: Boolean =
    HEIGHT >= votingStart && HEIGHT < voteDeadline

  // ---- Vote box creation validation ----
  // OUTPUTS(0) must be a properly structured userVote box
  val voteBoxValid: Boolean = {
    val voteBox: Box = OUTPUTS(0)

    // Script must be the userVote contract
    val correctScript: Boolean =
      blake2b256(voteBox.propositionBytes) == UserVoteScriptHash

    // Must carry vote NFT + vYOLO
    val correctTokens: Boolean =
      voteBox.tokens.size >= 2 &&
      voteBox.tokens(0)._1 == ValidVoteId &&
      voteBox.tokens(0)._2 == 1L &&
      voteBox.tokens(1)._1 == VYoloId &&
      voteBox.tokens(1)._2 > 0L

    // Vote direction must be valid (0 or 1)
    val validDirection: Boolean = {
      val dir: Long = voteBox.R4[Long].get
      dir == 0L || dir == 1L
    }

    // Cancellation unlock height must be reasonable
    val validCancelHeight: Boolean =
      voteBox.R7[Long].get >= HEIGHT + cancellationCooldown

    // Submission deadline must be before the vote deadline
    val validSubmissionDeadline: Boolean =
      voteBox.R8[Long].get <= voteDeadline

    correctScript && correctTokens && validDirection &&
    validCancelHeight && validSubmissionDeadline
  }

  // ================================================================
  // ENTRY
  // ================================================================
  sigmaProp(counterValid && withinVotingWindow && voteBoxValid)
}
