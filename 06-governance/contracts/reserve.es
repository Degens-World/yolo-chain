{
  // ================================================================
  // RESERVE CONTRACT v1.0
  // ================================================================
  // Part of the YOLO governance peg layer. Each reserve box holds vYOLO
  // proxy tokens plus a singleton reserve NFT. Paired 1:1 with a vault
  // box that holds the corresponding locked YOLO.
  //
  // The reserve is the vYOLO dispensary: users deposit YOLO into the
  // vault and receive vYOLO from the reserve (deposit), or return vYOLO
  // to the reserve and withdraw YOLO from the vault (redeem).
  //
  // Conservation is checked from BOTH sides (defense-in-depth). Both
  // vault.es and reserve.es must agree for any TX to validate.
  //
  // Box layout:
  //   SELF.value      = minimum box value (rent buffer only)
  //   SELF.tokens(0)  = (RESERVE_NFT_ID, 1L) — singleton marker
  //   SELF.tokens(1)  = (VYOLO_TOKEN_ID, N)  — available vYOLO
  //   SELF.tokens.size == 2  — no token contamination
  //   No registers used
  //
  // Conservation law (mirrors vault — both sides enforce):
  //   deltaVaultYolo + deltaReserveVYolo == 0
  //
  // TX layout: see vault.es header for full deposit/redeem shapes.
  // Reserve is always at INPUTS(1)/OUTPUTS(1) in the canonical layout.
  // ================================================================

  // ---- Compile-time constants (unique per reserve instance) ----
  val ReserveNftId: Coll[Byte] = fromBase16("RESERVE_NFT_PLACEHOLDER")
  val StateNftId: Coll[Byte]   = fromBase16("STATE_NFT_PLACEHOLDER")
  val VYoloId: Coll[Byte]      = fromBase16("VYOLO_TOKEN_PLACEHOLDER")

  // ---- Self integrity ----
  // Reserve box must hold exactly 2 tokens: reserve NFT + vYOLO.
  // Rejects any extra tokens that could contaminate the peg box.
  val selfValid: Boolean =
    SELF.tokens.size == 2 &&
    SELF.tokens(0)._1 == ReserveNftId &&
    SELF.tokens(0)._2 == 1L &&
    SELF.tokens(1)._1 == VYoloId

  // ---- Output (continued reserve) integrity ----
  // Successor reserve at OUTPUTS(1) must preserve script, NFT identity,
  // vYOLO token ID, and minimum box value.
  val out: Box = OUTPUTS(1)
  val outValid: Boolean =
    out.propositionBytes == SELF.propositionBytes &&
    out.tokens.size == 2 &&
    out.tokens(0)._1 == ReserveNftId &&
    out.tokens(0)._2 == 1L &&
    out.tokens(1)._1 == VYoloId &&
    out.value == SELF.value

  // ---- Locate paired vault by NFT filter ----
  // Exactly one vault input and one vault output must be present,
  // identified by their state NFT.
  val vaultIn: Coll[Box] = INPUTS.filter { (b: Box) =>
    b.tokens.size > 0 && b.tokens(0)._1 == StateNftId
  }
  val vaultOut: Coll[Box] = OUTPUTS.filter { (b: Box) =>
    b.tokens.size > 0 && b.tokens(0)._1 == StateNftId
  }
  val pairingValid: Boolean = vaultIn.size == 1 && vaultOut.size == 1

  // ---- Conservation (mirrors vault — defense-in-depth) ----
  // The change in reserve vYOLO must exactly offset the change in vault YOLO.
  val deltaReserveVYolo: Long = out.tokens(1)._2 - SELF.tokens(1)._2
  val deltaVaultYolo: Long    = vaultOut(0).value - vaultIn(0).value
  val conservation: Boolean   = deltaVaultYolo + deltaReserveVYolo == 0L

  // ---- Non-trivial TX ----
  val nonTrivial: Boolean = deltaReserveVYolo != 0L

  // ---- Single reserve input (defense in depth) ----
  // Ensures only one reserve box with this NFT is consumed per TX.
  val singleReserveInput: Boolean = INPUTS.filter { (b: Box) =>
    b.tokens.size > 0 && b.tokens(0)._1 == ReserveNftId
  }.size == 1

  sigmaProp(
    selfValid && outValid && pairingValid &&
    conservation && nonTrivial && singleReserveInput
  )
}
