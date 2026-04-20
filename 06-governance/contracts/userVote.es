{
  // ================================================================
  // USER VOTE CONTRACT v1.0 (DuckDAO port)
  // ================================================================
  // Per-voter box. Created when a user locks vYOLO to vote on a proposal.
  //
  // Ported from DuckPools treasury-system/voting/user-vote.
  //
  // Register layout:
  //   R4: Long           — vote direction (1L = yes, 0L = no)
  //   R5: Coll[Byte]     — proposal ID (links vote to specific proposal)
  //   R6: SigmaProp      — voter public key (for cancel authentication)
  //   R7: Long           — cancellation unlock height
  //   R8: Long           — submission deadline (must precede counter's nextVoteDeadline)
  //
  // Token layout:
  //   tokens(0) = (VALID_VOTE_ID, 1L)  — vote NFT (authenticates this as valid voter box)
  //   tokens(1) = (VYOLO_ID, N)        — locked vYOLO (voting power)
  //
  // Paths:
  //   1. Cancel  — voter reclaims vYOLO before counting (after cooldown)
  //   2. Submit  — consumed by counting contract, vote NFT BURNED
  // ================================================================

  // ---- Compile-time constants ----
  val ValidVoteId: Coll[Byte] = fromBase16("VALID_VOTE_PLACEHOLDER")
  val VYoloId: Coll[Byte]    = fromBase16("VYOLO_TOKEN_PLACEHOLDER")
  val CounterNftId: Coll[Byte] = fromBase16("COUNTER_NFT_PLACEHOLDER")

  // ---- Cancellation cooldown (SigmaChain 20s blocks) ----
  val cancellationCooldown: Long = 4320L  // 24 hours

  // ---- Read registers ----
  val voteDirection: Long       = SELF.R4[Long].get
  val proposalId: Coll[Byte]    = SELF.R5[Coll[Byte]].get
  val voterPk: SigmaProp        = SELF.R6[SigmaProp].get
  val cancelUnlockHeight: Long  = SELF.R7[Long].get
  val submissionDeadline: Long  = SELF.R8[Long].get

  // ---- Self integrity ----
  val selfValid: Boolean =
    SELF.tokens.size >= 2 &&
    SELF.tokens(0)._1 == ValidVoteId &&
    SELF.tokens(1)._1 == VYoloId

  val voterVYolo: Long = SELF.tokens(1)._2

  // ================================================================
  // PATH 1: CANCEL — voter reclaims vYOLO
  // ================================================================
  // After cooldown period AND before the counter box deadline.
  // Voter must sign (Sigma-protocol authentication via R6).
  // vYOLO returned to voter. Vote NFT returned to voter (not burned —
  // vote was never counted, so no anti-double-vote concern).
  val isCancel: Boolean = {
    // Cooldown passed
    val cooldownPassed: Boolean = HEIGHT >= cancelUnlockHeight

    // Counter box deadline not passed (vote hasn't been counted yet)
    val counterBox: Box = CONTEXT.dataInputs(0)
    val counterValid: Boolean =
      counterBox.tokens.size >= 1 &&
      counterBox.tokens(0)._1 == CounterNftId
    val counterDeadline: Long = counterBox.R4[Long].get
    val beforeDeadline: Boolean = HEIGHT < counterDeadline

    // vYOLO returned to voter's address
    val vyoloReturned: Boolean =
      OUTPUTS(0).tokens.size >= 1 &&
      OUTPUTS(0).tokens(0)._1 == VYoloId &&
      OUTPUTS(0).tokens(0)._2 >= voterVYolo

    cooldownPassed && counterValid && beforeDeadline && vyoloReturned
  }

  // ================================================================
  // PATH 2: SUBMIT — consumed by counting contract
  // ================================================================
  // During counting phase. Vote NFT is BURNED (removed from all
  // outputs) to prevent double-counting. vYOLO released to voter.
  // The counting contract (counting.es Phase 2) handles the tally
  // update — this contract only validates the spend conditions.
  val isSubmit: Boolean = {
    // Counter box must be in the transaction inputs (counting is consuming us)
    val counterInInputs: Boolean = INPUTS.exists { (b: Box) =>
      b.tokens.size >= 1 && b.tokens(0)._1 == CounterNftId
    }

    // Submission deadline not passed
    val withinDeadline: Boolean = HEIGHT <= submissionDeadline

    // Vote NFT must be burned — not in ANY output, at ANY token slot
    // (checking only tokens(0) would allow hiding the NFT at a later slot)
    val voteNftBurned: Boolean = OUTPUTS.forall { (o: Box) =>
      o.tokens.forall { (t: (Coll[Byte], Long)) => t._1 != ValidVoteId }
    }

    // vYOLO returned to voter's address
    val vyoloReturned: Boolean = OUTPUTS.exists { (o: Box) =>
      o.tokens.size >= 1 &&
      o.tokens(0)._1 == VYoloId &&
      o.tokens(0)._2 >= voterVYolo
    }

    counterInInputs && withinDeadline && voteNftBurned && vyoloReturned
  }

  // ================================================================
  // ENTRY
  // ================================================================
  // Cancel requires voter signature; Submit does not (counting bot submits)
  sigmaProp(selfValid) && (
    (voterPk && sigmaProp(isCancel)) ||
    sigmaProp(isSubmit)
  )
}
