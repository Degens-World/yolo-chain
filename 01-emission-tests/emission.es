{
  // ================================================================
  // EMISSION CONTRACT v1.1
  // ================================================================
  // Changes from v1.0 (Ergo-parity pass):
  //   - [Finding 1]  Treasury/LP value checks tightened from `>=` to `==`
  //                  to match Ergo's `EQ(coinsToIssue, consumedFromEmission)`
  //                  convention in emissionBoxProp.
  //   - [Gap A]      Added `heightIncreased` check: HEIGHT > SELF.creationInfo._1
  //                  Mirrors Ergo's belt-and-suspenders defense against
  //                  same-block spend of the emission box.
  //   - [Gap B]      Added `heightCorrect` check: OUTPUTS(0).creationInfo._1 == HEIGHT
  //                  Mirrors Ergo's creation-height lock on the successor box.
  //
  // Reference: ErgoScriptPredef.scala `emissionBoxProp` at commit bd1906e,
  // ScorexFoundation/sigmastate-interpreter.
  //
  // Box layout (unchanged from v1.0):
  //   SELF          = current emission box
  //   SELF.tokens(0)= singleton emission NFT (amount == 1)
  //   SELF.R4       = Coll[Byte]: blake2b256 hash of treasury script
  //   SELF.R5       = Coll[Byte]: blake2b256 hash of LP fund script
  //
  // Transaction layout (normal path):
  //   OUTPUTS(0)    = new emission box (value decreased by blockReward)
  //   OUTPUTS(1)    = treasury reward output
  //   OUTPUTS(2)    = LP fund reward output
  //   (miner reward enforced at consensus layer, not in this contract —
  //    deliberate design difference from Ergo; see ERGO_COMPARISON.md Gap C)
  //
  // Transaction layout (terminal path — exhaustion):
  //   When SELF.value < blockReward, all remaining value is distributed
  //   proportionally and the emission NFT is burned.
  // ================================================================

  // ---- Constants ----
  val blocksPerHalving: Int = 1577880     // ~1 year at 20s blocks
  val initialReward: Long   = 50000000000L // 50 coins in nanocoins
  val minReward: Long        = 1000000000L  // 1 coin — tail emission floor

  // ---- Singleton NFT identity ----
  val emissionNftId: Coll[Byte] = SELF.tokens(0)._1

  // ---- Height-based defenses (new in v1.1, match Ergo emissionBoxProp) ----
  // [Gap A] HEIGHT must have advanced since this box was created.
  // Prevents any same-block re-spend scenario; redundant with NFT singleton
  // but matches Ergo's belt-and-suspenders pattern.
  val heightIncreased: Boolean = HEIGHT > SELF.creationInfo._1

  // ---- Determine block reward from HEIGHT ----
  val halvings: Int = HEIGHT / blocksPerHalving

  val blockReward: Long = {
    val computed: Long = if (halvings <= 0) initialReward
      else if (halvings == 1) initialReward / 2L
      else if (halvings == 2) initialReward / 4L
      else if (halvings == 3) initialReward / 8L
      else if (halvings == 4) initialReward / 16L
      else if (halvings == 5) initialReward / 32L
      else minReward

    // Floor: never below minReward
    if (computed > minReward) computed else minReward
  }

  // ---- Split calculation ----
  // 85% miner (consensus layer), 10% treasury, 5% LP (remainder absorbed by miner residual)
  val treasuryReward: Long = blockReward * 10L / 100L
  val lpReward: Long       = blockReward * 5L / 100L

  // ---- Register-stored destination hashes ----
  val treasuryScriptHash: Coll[Byte] = SELF.R4[Coll[Byte]].get
  val lpScriptHash: Coll[Byte]       = SELF.R5[Coll[Byte]].get

  // ================================================================
  // PATH 1: NORMAL EMISSION (SELF.value >= blockReward)
  // ================================================================
  val normalPath: Boolean = {
    val sufficientFunds: Boolean = SELF.value >= blockReward

    // -- Output 0: new emission box --
    val nextBox = OUTPUTS(0)
    val nftPreserved: Boolean = {
      nextBox.tokens.size > 0 &&
      nextBox.tokens(0)._1 == emissionNftId &&
      nextBox.tokens(0)._2 == 1L
    }
    val scriptPreserved: Boolean = nextBox.propositionBytes == SELF.propositionBytes
    val valueCorrect: Boolean    = nextBox.value == SELF.value - blockReward
    val registersPreserved: Boolean = {
      nextBox.R4[Coll[Byte]].get == treasuryScriptHash &&
      nextBox.R5[Coll[Byte]].get == lpScriptHash
    }
    // [Gap B] New emission box must declare creation height equal to current HEIGHT.
    val heightCorrect: Boolean = nextBox.creationInfo._1 == HEIGHT

    val validEmissionBox: Boolean =
      nftPreserved && scriptPreserved && valueCorrect && registersPreserved && heightCorrect

    // -- Output 1: treasury --  [Finding 1: == not >=]
    val validTreasury: Boolean = {
      OUTPUTS(1).value == treasuryReward &&
      blake2b256(OUTPUTS(1).propositionBytes) == treasuryScriptHash
    }

    // -- Output 2: LP fund --  [Finding 1: == not >=]
    val validLP: Boolean = {
      OUTPUTS(2).value == lpReward &&
      blake2b256(OUTPUTS(2).propositionBytes) == lpScriptHash
    }

    sufficientFunds && validEmissionBox && validTreasury && validLP
  }

  // ================================================================
  // PATH 2: TERMINAL EMISSION (SELF.value < blockReward)
  // ================================================================
  // When the emission box can't pay a full block reward,
  // distribute all remaining value proportionally and end emission.
  // The emission NFT is burned (not preserved in any output).
  val terminalPath: Boolean = {
    val insufficient: Boolean = SELF.value < blockReward
    val remaining: Long       = SELF.value

    // Proportional split of whatever's left
    val termTreasury: Long = remaining * 10L / 100L
    val termLP: Long       = remaining * 5L / 100L
    // Miner gets the rest via consensus layer

    // [Finding 1: == not >=]
    val validTermTreasury: Boolean = {
      OUTPUTS(0).value == termTreasury &&
      blake2b256(OUTPUTS(0).propositionBytes) == treasuryScriptHash
    }
    // [Finding 1: == not >=]
    val validTermLP: Boolean = {
      OUTPUTS(1).value == termLP &&
      blake2b256(OUTPUTS(1).propositionBytes) == lpScriptHash
    }

    // Explicitly verify NFT is destroyed — cannot be smuggled into any output
    val nftBurned: Boolean = OUTPUTS.forall { (o: Box) =>
      o.tokens.forall { (t: (Coll[Byte], Long)) => t._1 != emissionNftId }
    }

    insufficient && validTermTreasury && validTermLP && nftBurned
  }

  // [Gap A] heightIncreased is required on EVERY spend (both paths).
  sigmaProp(heightIncreased && (normalPath || terminalPath))
}
