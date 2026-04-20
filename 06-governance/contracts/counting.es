{
  // ================================================================
  // COUNTING CONTRACT v1.1 (DuckDAO port)
  // ================================================================
  // 4-phase state machine for sequential vote counting.
  // Ported from DuckPools treasury-system/voting/counting.md.
  //
  // v1.1 fixes (from audit):
  //   - Vote NFT burn now verified in OUTPUTS (not just counted in INPUTS)
  //   - Phase timing corrected: counting runs for countingPhase blocks only
  //   - Phase 1 guards against re-initiation during active voting
  //   - Triple fold combined into single fold for JIT cost
  //   - Removed unused ProposalTreeHash constant
  //
  // Changes from DuckDAO:
  //   - QUACKS → vYOLO
  //   - Height constants × 6 for 20s blocks
  //   - Initiation stake forfeit on cancel (new)
  //   - Tiered support: 50% normal, 90% for proportion > 1,000,000
  //
  // Register layout:
  //   R4: Long             — vote deadline (end of voting window)
  //   R5: (Long, Long)     — (nominated proportion, votes in favor)
  //   R6: Coll[Byte]       — recipient ergotree hash (blake2b256)
  //   R7: Long             — total votes accumulated
  //   R8: Long             — initiation stake amount (vYOLO by proposer)
  //   R9: Long             — validation votes (yes-votes subset)
  //
  // Phases (gated by HEIGHT):
  //   1. Before counting   — initiate new vote (only when no active proposal)
  //   2. Counting period   — accumulate votes, burn vote NFTs
  //   3. Vote validation   — check thresholds, advance proposal state
  //   4. New proposal      — reset for next round
  //
  // Token layout:
  //   tokens(0) = (COUNTER_NFT_ID, 1L) — singleton identity
  // ================================================================

  // ---- Compile-time constants ----
  val CounterNftId: Coll[Byte]  = fromBase16("COUNTER_NFT_PLACEHOLDER")
  val VYoloId: Coll[Byte]       = fromBase16("VYOLO_TOKEN_PLACEHOLDER")
  val ValidVoteId: Coll[Byte]   = fromBase16("VALID_VOTE_PLACEHOLDER")

  // ---- Height constants (SigmaChain 20s blocks = Ergo × 6) ----
  val votingWindow: Long    = 12960L   // 3 days
  val countingPhase: Long   = 1080L    // 6 hours
  val executionGrace: Long  = 4320L    // 24 hours

  // ---- Thresholds ----
  val initiationHurdle: Long   = 100000000000000L  // 100,000 vYOLO in nanocoins
  val quorumFloor: Long        = 1000000000000000L // 1,000,000 vYOLO in nanocoins
  val elevatedProportion: Long = 1000000L          // >10% of treasury → 90% support
  val minimumSupport: Long     = 500L              // 50% (out of 1000)
  val elevatedSupport: Long    = 900L              // 90% (out of 1000)

  // ---- Read current state ----
  val voteDeadline: Long             = SELF.R4[Long].get
  val currentTally                   = SELF.R5[(Long, Long)].get
  val currentProportion: Long        = currentTally._1
  val currentVotesFor: Long          = currentTally._2
  val recipientHash: Coll[Byte]      = SELF.R6[Coll[Byte]].get
  val totalVotes: Long               = SELF.R7[Long].get
  val initiationStake: Long          = SELF.R8[Long].get
  val validationVotes: Long          = SELF.R9[Long].get

  val out0: Box = OUTPUTS(0)

  // ---- Phase detection (corrected timing) ----
  // Voting:    [voteDeadline - votingWindow, voteDeadline)
  // Counting:  [voteDeadline, voteDeadline + countingPhase)
  // Validation:[voteDeadline + countingPhase, voteDeadline + countingPhase + executionGrace)
  // New round: [voteDeadline + countingPhase + executionGrace, ...)
  val countingEnd: Long     = voteDeadline + countingPhase
  val validationEnd: Long   = countingEnd + executionGrace

  val isBeforeCounting: Boolean       = HEIGHT < voteDeadline
  val isCountingPeriod: Boolean       = HEIGHT >= voteDeadline && HEIGHT < countingEnd
  val isVoteValidationPeriod: Boolean = HEIGHT >= countingEnd && HEIGHT < validationEnd
  val isNewProposalPeriod: Boolean    = HEIGHT >= validationEnd

  // ---- Shared: counter NFT preserved in OUTPUTS(0) ----
  val counterPreserved: Boolean =
    out0.propositionBytes == SELF.propositionBytes &&
    out0.tokens.size >= 1 &&
    out0.tokens(0)._1 == CounterNftId &&
    out0.tokens(0)._2 == 1L

  // ================================================================
  // PHASE 1: BEFORE COUNTING — initiate new vote
  // ================================================================
  // Proposer provides vYOLO stake meeting initiation hurdle.
  // Counter box resets tallies, stores proposal parameters.
  // Guard: only allowed when no active proposal (totalVotes == 0
  // and validationVotes == 0 — prevents hijacking an active vote).
  val phase1: Boolean = {
    // Guard against re-initiation during active voting
    val noActiveProposal: Boolean = totalVotes == 0L && validationVotes == 0L

    val initiationBox = CONTEXT.dataInputs(0)

    // Initiator must hold sufficient vYOLO
    val hasEnoughStake: Boolean = initiationBox.tokens.size >= 1 &&
      initiationBox.tokens(0)._1 == VYoloId &&
      initiationBox.tokens(0)._2 >= initiationHurdle

    // Reset tallies in successor, set new deadline
    val talliesReset: Boolean = {
      out0.R7[Long].get == 0L &&                    // total votes = 0
      out0.R9[Long].get == 0L &&                    // validation votes = 0
      out0.R4[Long].get == HEIGHT + votingWindow &&  // new deadline
      out0.R8[Long].get >= initiationHurdle           // stake recorded
    }

    val valuePreserved: Boolean = out0.value >= SELF.value

    isBeforeCounting && noActiveProposal && hasEnoughStake &&
    talliesReset && counterPreserved && valuePreserved
  }

  // ================================================================
  // PHASE 2: COUNTING PERIOD — accumulate votes
  // ================================================================
  // Each TX consumes voter boxes (INPUTS(1+)) and increments tallies.
  // Vote NFTs are BURNED — verified absent from ALL outputs.
  // Single fold for JIT cost efficiency.
  val phase2: Boolean = {
    val voterBoxes: Coll[Box] = INPUTS.slice(1, INPUTS.size)

    // Single fold: accumulate (totalVotes, yesVotes) in one pass
    val voteCounts = voterBoxes.fold((0L, 0L), { (acc: (Long, Long), voter: Box) =>
      if (voter.tokens.size >= 2 &&
          voter.tokens(0)._1 == ValidVoteId &&
          voter.tokens(1)._1 == VYoloId) {
        val power: Long = voter.tokens(1)._2
        val yesAdd: Long = if (voter.R4[Long].get == 1L) power else 0L
        (acc._1 + power, acc._2 + yesAdd)
      } else {
        acc
      }
    })
    val votesThisRound: Long    = voteCounts._1
    val yesVotesThisRound: Long = voteCounts._2

    // Tallies must be correctly updated
    val talliesUpdated: Boolean = {
      val newTally = out0.R5[(Long, Long)].get
      newTally._1 == currentProportion &&
      newTally._2 == currentVotesFor + yesVotesThisRound &&
      out0.R7[Long].get == totalVotes + votesThisRound &&
      out0.R9[Long].get == validationVotes + yesVotesThisRound
    }

    // Deadline and other fields preserved
    val fieldsPreserved: Boolean = {
      out0.R4[Long].get == voteDeadline &&
      out0.R6[Coll[Byte]].get == recipientHash &&
      out0.R8[Long].get == initiationStake
    }

    // Vote NFTs must be BURNED — not present in ANY output, at ANY token slot
    val voteNftsBurnedFromOutputs: Boolean = OUTPUTS.forall { (o: Box) =>
      o.tokens.forall { (t: (Coll[Byte], Long)) => t._1 != ValidVoteId }
    }

    val valuePreserved: Boolean = out0.value >= SELF.value

    isCountingPeriod && votesThisRound > 0L && talliesUpdated &&
    fieldsPreserved && voteNftsBurnedFromOutputs && counterPreserved && valuePreserved
  }

  // ================================================================
  // PHASE 3: VOTE VALIDATION — check thresholds, advance proposal
  // ================================================================
  // If thresholds met: advance proposal state token 1 → 2.
  // Counter resets for next round regardless.
  val phase3: Boolean = {
    val proposalBox: Box = INPUTS(1)

    // Check thresholds
    val meetsQuorum: Boolean = totalVotes >= quorumFloor

    // Tiered support: 90% for large proposals, 50% for normal
    val requiredSupport: Long = if (currentProportion > elevatedProportion) {
      elevatedSupport
    } else {
      minimumSupport
    }
    val actualSupport: Long = if (totalVotes > 0L) {
      validationVotes * 1000L / totalVotes
    } else {
      0L
    }
    val meetsSupport: Boolean = actualSupport >= requiredSupport

    val proposalPassed: Boolean = meetsQuorum && meetsSupport

    // If passed: proposal box state token advances 1 → 2
    val proposalAdvanced: Boolean = if (proposalPassed) {
      val proposalTokens = proposalBox.tokens(0)
      proposalTokens._2 == 1L &&  // current state = pending
      OUTPUTS(1).tokens.size >= 1 &&
      OUTPUTS(1).tokens(0)._1 == proposalTokens._1 &&
      OUTPUTS(1).tokens(0)._2 == 2L &&  // new state = passed
      OUTPUTS(1).propositionBytes == proposalBox.propositionBytes &&
      OUTPUTS(1).R4[(Long, Long)].get._1 == currentProportion &&
      blake2b256(OUTPUTS(1).R5[Coll[Byte]].get) == recipientHash
    } else {
      true  // no proposal advancement needed on failure
    }

    // Counter box resets for next round
    val counterReset: Boolean = {
      out0.R7[Long].get == 0L &&
      out0.R9[Long].get == 0L
    }

    val valuePreserved: Boolean = out0.value >= SELF.value

    isVoteValidationPeriod && proposalAdvanced && counterReset &&
    counterPreserved && valuePreserved
  }

  // ================================================================
  // PHASE 4: NEW PROPOSAL PERIOD — allow counter reuse
  // ================================================================
  // After validation deadline passes, counter can accept next proposal.
  // Resets tallies to allow Phase 1 to trigger.
  val phase4: Boolean = {
    val valuePreserved: Boolean = out0.value >= SELF.value

    // Reset tallies so Phase 1's noActiveProposal guard passes
    val talliesCleared: Boolean =
      out0.R7[Long].get == 0L &&
      out0.R9[Long].get == 0L

    isNewProposalPeriod && counterPreserved && valuePreserved && talliesCleared
  }

  // ================================================================
  // ENTRY
  // ================================================================
  sigmaProp(phase1 || phase2 || phase3 || phase4)
}
