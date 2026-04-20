{
  // ================================================================
  // GOVERNANCE TREASURY CONTRACT v1.1
  // ================================================================
  // Governance-controlled treasury — replaces the 2-of-3 multisig
  // treasury_governance.es after migration. Activated when the multisig
  // signers execute migration path (treasury_governance.es Path 6+7),
  // transferring the governance NFT to this contract.
  //
  // This contract has NO multisig, NO admin keys. All spending is
  // controlled by the on-chain vYOLO voting system.
  //
  // Distribution model: proportion-based (DuckDAO convention).
  // Proposals specify a fraction (numerator / 10,000,000) of the
  // treasury's total YOLO value. Inherently safe — can never propose
  // more than treasury holds.
  //
  // CRITICAL: Uses split-math pattern to avoid i64 overflow.
  // Naive `value * proportion / denom` overflows at YOLO's nanocoin
  // scale. See implementation plan §7.8.
  //
  // Box layout:
  //   SELF.value      = treasury YOLO (nanocoins)
  //   SELF.tokens(0)  = (TREASURY_NFT_ID, 1L) — singleton identity
  //   No registers used (stateless treasury)
  //
  // Paths:
  //   1. Deposit       — anyone can add YOLO (value monotonicity)
  //   2. Withdrawal    — governance-approved proportional disbursement
  //   3. New-treasury  — full migration to new governance contract
  //
  // Integration:
  //   - Withdrawal/migration requires a PASSED PROPOSAL BOX in
  //     INPUTS(0) — identified by its state token at qty == 2
  //     (set by counting.es Phase 3 on successful validation).
  //   - Proposal box R4 = (proportion, 0L), R5 = recipient hash.
  //   - New-treasury-mode: proportion == 10,000,000 (100%).
  //
  // v1.1 changes:
  //   - Fixed: treasury checks proposal state token (qty=2), not
  //     counter token. Counter NFT is singleton (always qty=1).
  //   - Fixed: removed dead code (noCounterToken variable).
  //   - Deposit path distinguished by SELF being at INPUTS(0).
  // ================================================================

  // ---- Compile-time constants ----
  val TreasuryNftId: Coll[Byte]    = fromBase16("TREASURY_NFT_PLACEHOLDER")
  val ProposalTokenId: Coll[Byte]  = fromBase16("PROPOSAL_TOKEN_PLACEHOLDER")
  val Denom: Long = 10000000L  // proportion denominator (DuckDAO convention)

  val out0: Box = OUTPUTS(0)

  // ---- Self integrity ----
  val selfValid: Boolean =
    SELF.tokens.size >= 1 &&
    SELF.tokens(0)._1 == TreasuryNftId &&
    SELF.tokens(0)._2 == 1L

  // ================================================================
  // PATH 1: DEPOSIT — anyone can add YOLO to treasury
  // ================================================================
  // Treasury is SELF at INPUTS(0). Value can only increase.
  // Script and NFT preserved. Distinguished from withdrawal by
  // checking that SELF is INPUTS(0) (no proposal box at INPUTS(0)).
  val isDeposit: Boolean = {
    val selfIsFirstInput: Boolean = INPUTS(0).id == SELF.id

    val scriptPreserved: Boolean = out0.propositionBytes == SELF.propositionBytes
    val nftPreserved: Boolean =
      out0.tokens.size >= 1 &&
      out0.tokens(0)._1 == TreasuryNftId &&
      out0.tokens(0)._2 == 1L
    val valueGrown: Boolean = out0.value >= SELF.value

    selfIsFirstInput && scriptPreserved && nftPreserved && valueGrown
  }

  // ================================================================
  // PATH 2: WITHDRAWAL — governance-approved proportional disbursement
  // ================================================================
  // INPUTS(0) must be a PASSED PROPOSAL BOX — identified by carrying
  // the proposal state token with quantity == 2 (proof of passed vote).
  // Proposal box R4 = (proportion, 0L), R5 = recipient ergotree hash.
  // ---- Shared proposal box reference (INPUTS(0) in withdrawal/migration) ----
  // CRITICAL: Register access must be guarded to avoid eager ValDef crash.
  // When deposit path is taken, INPUTS(0) = treasury (no R4/R5 registers).
  // The Scala compiler hoists shared ValDefs — `.get` on missing registers
  // crashes before path selection. Guard with token check first.
  val proposalBox: Box = INPUTS(0)
  val hasProposalToken: Boolean =
    proposalBox.tokens.size >= 1 &&
    proposalBox.tokens(0)._1 == ProposalTokenId &&
    proposalBox.tokens(0)._2 == 2L

  val isWithdrawal: Boolean = {
    if (hasProposalToken) {
      // Safe to read registers only after confirming proposal token present
      val proposalTuple = proposalBox.R4[(Long, Long)].get
      val proportion: Long = proposalTuple._1
      val recipientHash: Coll[Byte] = proposalBox.R5[Coll[Byte]].get

      // Proportion must be < Denom (not 100% — that's new-treasury-mode)
      val isPartialWithdrawal: Boolean = proportion < Denom && proportion > 0L

      // ---- Split-math: overflow-safe proportional calculation ----
      // NEVER: val awarded = SELF.value * proportion / Denom  (OVERFLOWS)
      val wholePart: Long     = (SELF.value / Denom) * proportion
      val remainderPart: Long = ((SELF.value % Denom) * proportion) / Denom
      val awarded: Long       = wholePart + remainderPart

      // Treasury successor: same script, same NFT, reduced value
      val treasuryPreserved: Boolean =
        out0.propositionBytes == SELF.propositionBytes &&
        out0.tokens.size >= 1 &&
        out0.tokens(0)._1 == TreasuryNftId &&
        out0.tokens(0)._2 == 1L &&
        out0.value >= SELF.value - awarded

      // Recipient output matches proposal
      val recipientValid: Boolean =
        blake2b256(OUTPUTS(1).propositionBytes) == recipientHash &&
        OUTPUTS(1).value >= awarded

      isPartialWithdrawal && treasuryPreserved && recipientValid
    } else {
      false
    }
  }

  // ================================================================
  // PATH 3: NEW-TREASURY-MODE — full migration to new governance
  // ================================================================
  // proportion == 10,000,000 (Denom) means 100% of treasury.
  // Transfers all value + NFT to new contract specified by recipient.
  // This is the on-chain governance upgrade mechanism.
  // Triggers 90% elevated support threshold in counting.es
  // (since proportion > 1,000,000).
  val isNewTreasury: Boolean = {
    if (hasProposalToken) {
      val proposalTuple = proposalBox.R4[(Long, Long)].get
      val proportion: Long = proposalTuple._1
      val recipientScript: Coll[Byte] = proposalBox.R5[Coll[Byte]].get

      val isFullTransfer: Boolean = proportion == Denom

      // New treasury gets ALL value and the NFT
      val newTreasuryValid: Boolean =
        out0.value >= SELF.value &&
        out0.tokens.size >= 1 &&
        out0.tokens(0)._1 == TreasuryNftId &&
        out0.tokens(0)._2 == 1L &&
        blake2b256(out0.propositionBytes) == recipientScript

      isFullTransfer && newTreasuryValid
    } else {
      false
    }
  }

  // ================================================================
  // ENTRY
  // ================================================================
  sigmaProp(selfValid && (isDeposit || isWithdrawal || isNewTreasury))
}
