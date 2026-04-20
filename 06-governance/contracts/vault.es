{
  // ================================================================
  // VAULT CONTRACT v1.0
  // ================================================================
  // Part of the YOLO governance peg layer. Each vault box holds locked
  // YOLO (native coin) plus a singleton state NFT. Paired 1:1 with a
  // reserve box that holds the corresponding vYOLO proxy tokens.
  //
  // CONSTITUTIONAL INVARIANT: This vault has NO governance-controlled
  // spend path. Governance proposals can only disburse from the treasury
  // box, never from vaults. The peg (vYOLO supply <= locked YOLO) is
  // structurally enforced, not governable.
  //
  // 5 independent vault/reserve pairs exist for concurrency. Each pair
  // has its own state NFT and reserve NFT, baked in at compile time.
  //
  // Box layout:
  //   SELF.value      = locked YOLO (nanocoins)
  //   SELF.tokens(0)  = (STATE_NFT_ID, 1L)  — singleton marker
  //   SELF.tokens.size == 1  — no token contamination
  //   No registers used
  //
  // Conservation law (enforced from both sides — vault AND reserve):
  //   deltaVaultYolo + deltaReserveVYolo == 0
  //   i.e., YOLO deposited into vault == vYOLO withdrawn from reserve
  //         YOLO withdrawn from vault == vYOLO deposited into reserve
  //
  // TX layout (deposit):
  //   INPUTS(0)  = Vault_i   [state_nft, V YOLO]
  //   INPUTS(1)  = Reserve_i [reserve_nft, R vYOLO]
  //   INPUTS(2+) = User funds
  //   OUTPUTS(0) = Vault_i'  [state_nft, (V + X) YOLO]
  //   OUTPUTS(1) = Reserve_i'[reserve_nft, (R - X) vYOLO]
  //   OUTPUTS(2) = User receipt [X vYOLO]
  //
  // TX layout (redeem): mirror of deposit (vault decreases, reserve increases)
  //
  // Reference: DuckDAO governance (DuckPools) — no direct DuckDAO equivalent
  // for the peg layer; this is new for YOLO's native-coin governance.
  // ================================================================

  // ---- Compile-time constants (unique per vault instance) ----
  val StateNftId: Coll[Byte]   = fromBase16("STATE_NFT_PLACEHOLDER")
  val ReserveNftId: Coll[Byte] = fromBase16("RESERVE_NFT_PLACEHOLDER")

  // ---- Self integrity ----
  // Vault box must hold exactly 1 token: the state NFT singleton.
  // Rejects any extra tokens that could contaminate the peg box.
  val selfValid: Boolean =
    SELF.tokens.size == 1 &&
    SELF.tokens(0)._1 == StateNftId &&
    SELF.tokens(0)._2 == 1L

  // ---- Output (continued vault) integrity ----
  // Successor vault at OUTPUTS(0) must preserve script and NFT identity.
  val out: Box = OUTPUTS(0)
  val outValid: Boolean =
    out.propositionBytes == SELF.propositionBytes &&
    out.tokens.size == 1 &&
    out.tokens(0)._1 == StateNftId &&
    out.tokens(0)._2 == 1L

  // ---- Locate paired reserve by NFT filter ----
  // Exactly one reserve input and one reserve output must be present,
  // identified by their reserve NFT.
  val reserveIn: Coll[Box] = INPUTS.filter { (b: Box) =>
    b.tokens.size > 0 && b.tokens(0)._1 == ReserveNftId
  }
  val reserveOut: Coll[Box] = OUTPUTS.filter { (b: Box) =>
    b.tokens.size > 0 && b.tokens(0)._1 == ReserveNftId
  }
  val pairingValid: Boolean = reserveIn.size == 1 && reserveOut.size == 1

  // ---- Conservation ----
  // The change in vault YOLO must exactly offset the change in reserve vYOLO.
  // Deposit: vault gains X YOLO, reserve loses X vYOLO (delta sum = 0)
  // Redeem:  vault loses X YOLO, reserve gains X vYOLO (delta sum = 0)
  val deltaVaultYolo: Long    = out.value - SELF.value
  val deltaReserveVYolo: Long = reserveOut(0).tokens(1)._2 - reserveIn(0).tokens(1)._2
  val conservation: Boolean   = deltaVaultYolo + deltaReserveVYolo == 0L

  // ---- Non-trivial TX ----
  // Prevent no-op TXs that waste chain resources.
  val nonTrivial: Boolean = deltaVaultYolo != 0L

  // ---- Single vault input (defense in depth) ----
  // Ensures only one vault box with this NFT is consumed per TX.
  // Prevents multi-vault merging attacks.
  val singleVaultInput: Boolean = INPUTS.filter { (b: Box) =>
    b.tokens.size > 0 && b.tokens(0)._1 == StateNftId
  }.size == 1

  sigmaProp(
    selfValid && outValid && pairingValid &&
    conservation && nonTrivial && singleVaultInput
  )
}
