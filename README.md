# Motor Trail — prototype 4x4 Bevy + Rapier

Prototype **desktop** en Rust : une petite vallée low-poly procédurale, avec montée sinueuse, arbres, rochers, refuge et véhicule pilotable. **Aucun asset externe n'est nécessaire** : les décors et le 4x4 sont construits à partir de primitives géométriques.

## Démarrer

Installer [Rust stable](https://rustup.rs/), puis exécuter :

```bash
cargo run --release
```

La première compilation de Bevy peut être longue. Le prototype ouvre une fenêtre 3D.

## Commandes

| Touche | Action |
| --- | --- |
| W / Z / flèche haut | Accélérer |
| S / flèche bas | Reculer et freiner |
| A / Q / flèche gauche | Braquer à gauche |
| D / flèche droite | Braquer à droite |
| Espace | Frein |
| L | Basculer entre gamme normale et courte |
| R | Réinitialiser la position du véhicule |
| Échap | Quitter |

**Objectif** : atteindre le refuge en haut de la piste, ou explorer librement la forêt.

## Contenu

- Terrain procédural d'environ 120 × 165 m avec pente et chemin de terre.
- Arbres, rochers et cabane stylisés avec collisions Rapier.
- Châssis dynamique Rapier et 4 suspensions raycast appliquant des forces aux points de contact.
- Motricité, friction latérale et gamme courte simplifiées.
- Caméra suiveuse, HUD de vitesse, arrivée au refuge.

## Limites connues

Il s'agit d'un **prototype semi-arcade**, pas d'une simulation complète de pneus. Les roues ne sont pas des rigid bodies indépendants et n'animent pas encore leur débattement. Pas de treuil, de boue déformable, de cargaisons ni de missions multiples. Le comportement de conduite reste à régler avec des tests de jeu.

## Vérification

```bash
cargo check
cargo test
```

Le dépôt inclut un workflow GitHub Actions pour vérifier le build et les tests sur Linux. Versions de départ : **Bevy 0.19.0**, **bevy_rapier3d 0.35.0**.

## Prochaines étapes

1. Ajuster la physique, la garde au sol, la suspension et l'adhérence.
2. Animer les roues avec le débattement raycast.
3. Ajouter un treuil entre véhicule et arbres.
4. Ajouter les premières missions de livraison.
