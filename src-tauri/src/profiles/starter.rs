pub(super) const NEW_PROFILE_TEMPLATE: &str = r#"# ==================== TRASH HIDES ====================

"Gold" hide

# Low-tier base items (pre-sacred)
1 2 3 4 low normal superior rare hide
magic hide

sacred low normal superior magic hide

# show sacred eth
sacred superior eth notify

# Potions
".*Healing Potion$" hide
".*Mana Potion$" hide

# ==================== ANNOUNCEMENTS ====================

# Gems
"Onyx|Ruby|Topaz|Diamond|Rainbow Stone|Sapphire|Emerald|Amber|Bloodstone|Amethyst|Skull|Turquoise" hide
"^Perfect" show

# Jewelry
"Jewel|Quiver" rare notify

"Amulet$" rare {[3-9] to All Skills} stat notify
"Ring$" rare {[1-2] to All Skills} stat notify

# Uniques and sets
unique notify
set notify map

# Sacred uniques
sacred unique notify map

# angelics
angelic notify

# Mastercrafted items
master show notify map purple sound1

# Runes
#"^(El|Eld|Tir|Nef|Eth|Ith|Tal|Ral|Ort|Thul|Amn|Sol|Shael|Dol|Hel|Io|Lum|Ko|Fal|Lem|Pul|Um|Mal|Ist|Gul|Vex|Ohm|Lo|Sur|Ber|Jah|Cham|Zod) Rune$"
"^(Ber|Jah|Cham|Zod) Rune$" notify

[notify map sound3] {
  "Great Rune"
  "Enchanted Rune"
  "Elemental Rune"
  "Container" purple
  "Runestone|Essence$" red
}

# Consumables
[notify] {
  "Mystic Orb"
  "Arcane (Shard|Crystal|Cluster)"
  "Heavenly|Crate"
  "Shrine \(10"
  "Vessel"
}

# Essences, relics, arcane materials, enchant scrolls
[notify map sound2] {
  "Essence"
  "Corrupted (Shard|Crystal|Cluster)"
  "Enchant Scroll"
}

# Reagents
[notify] {
  "Enchanting"
  "Mystic Dye"
  "Treasure"
  "Item Design"
}

# Oils and special consumables
[notify map sound2] {
  "Oil of Augmentation"
  "Oil of Conjuration"
  "Oil of Greater Luck"
  "Oil of Intensity"
  "Belladonna Extract"
  "Heavenly Soul"
}

# Quest items
[notify orange map] {
  "Ring of the Five"
  "Sigil$"
  "Tome of Possession"
  "Tenet"
  "Book of Cain"
  "Positronic Brain"
}

"Quest Item|Cube Reagent" notify orange map

"Riftstone" red notify map
"Relic" red notify map sound3

# Trophies, effigies, emblems
[notify map] {
  "Trophy"
  "Occult Effigy"
  "Emblem of"
}

# Cycles
[notify] {
  "Cycle"
  "Medium Cycle" sound1
  "Large Cycle" sound2
  "Golden Cycle" red sound3 map
}

# Signets
[notify orange map] {
  "Signet of Learning"
}

# Charms
[stat green notify map] {
  "Zakarum's Ear|Visions of Akarat|Bone Chimes|Spirit Trance Herb|Soul of Kabraxis|Fool's Gold"
  "Sunstone of the Twin Seas|The Butcher's Tooth|Optical Detector|Laser Focus Crystal|Scroll of Kings|Moon of the Spider|Horazon's Focus|Six Angel Bag"
  "Sacred Worldstone Key|The Black Road|Azmodan's Heart|Hammer of the Taan Judges|Sunstone of the Gods|Spirit of Creation|Idol of Vanity|Silver Seal of Ureh"
  "Crystalline Flame Medallion|Legacy of Blood|Weather Control|Demonsbane|Umbaru Treasure|Xazax's Illusion|The Ancient Repositories|The Sleep|Dragon Claw|Neutrality Pact"
  "Eternal Bone Pile|Corrupted Wormhole|Cold Fusion Schematics|Lylia's Curse|Astrogha's Venom Stinger|The Glorious Book of Median|Books of Kalan|Vial of Elder Blood"
}

# Stat group examples
# [rare angelic stat notify] {
#   {focus} {enemy fire}
#   {focus} {speeds}
#   {speeds} {enemy fire}
#   "Light Plated" {speeds} {focus}
#   {Frozen Soul}
# }
#
# "Amulet" rare {[3-9] to All Skills} {focus} {enemy fire} stat notify
# "Arrow Quiver" {druid} rare stat notify
"#;
