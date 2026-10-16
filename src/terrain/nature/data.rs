use super::NatureAsset;

macro_rules! catalogue {
    ($(($variant:ident, $id:literal, $name:literal, $category:literal)),+ $(,)?) => {
        impl NatureAsset {
            pub const ALL: [Self; 23] = [$(Self::$variant),+];

            pub fn id(self) -> &'static str {
                match self { $(Self::$variant => $id),+ }
            }

            pub fn name(self) -> &'static str {
                match self { $(Self::$variant => $name),+ }
            }

            pub fn category(self) -> &'static str {
                match self { $(Self::$variant => $category),+ }
            }

            pub fn source_bytes(self) -> &'static [u8] {
                match self {
                    $(Self::$variant => include_bytes!(concat!("../../../assets/3D/assets/", $id, ".glb"))),+
                }
            }
        }
    };
}

catalogue! {
    (TreeOakRound, "tree_oak_round", "Chene rond", "Arbres"),
    (TreeOakWide, "tree_oak_wide", "Chene large", "Arbres"),
    (TreeOakTall, "tree_oak_tall", "Chene haut", "Arbres"),
    (TreeYoung, "tree_young", "Jeune arbre", "Arbres"),
    (TreePineSmall, "tree_pine_small", "Petit pin", "Arbres"),
    (TreePineMedium, "tree_pine_medium", "Pin moyen", "Arbres"),
    (TreePineTall, "tree_pine_tall", "Grand pin", "Arbres"),
    (RockBoulderSmall, "rock_boulder_small", "Petit rocher", "Rochers"),
    (RockBoulderMedium, "rock_boulder_medium", "Rocher moyen", "Rochers"),
    (RockBoulderLarge, "rock_boulder_large", "Grand rocher", "Rochers"),
    (RockSlabFlat, "rock_slab_flat", "Dalle plate", "Rochers"),
    (RockCluster, "rock_cluster", "Groupe de rochers", "Rochers"),
    (BushLow, "bush_low", "Buisson bas", "Buissons"),
    (BushRound, "bush_round", "Buisson rond", "Buissons"),
    (BushWild, "bush_wild", "Buisson sauvage", "Buissons"),
    (GrassShort, "grass_short", "Herbe courte", "Herbes"),
    (GrassTall, "grass_tall", "Herbe haute", "Herbes"),
    (GrassWild, "grass_wild", "Herbe sauvage", "Herbes"),
    (FernSmall, "fern_small", "Petite fougere", "Fougeres"),
    (FernLarge, "fern_large", "Grande fougere", "Fougeres"),
    (LogFallenSmall, "log_fallen_small", "Petit tronc couche", "Bois"),
    (LogFallenLarge, "log_fallen_large", "Grand tronc couche", "Bois"),
    (StumpOld, "stump_old", "Vieille souche", "Bois"),
}
