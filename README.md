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
| Clic droit maintenu + mouvement de souris | Faire tourner la caméra à 360° autour du véhicule |
| Molette de souris / défilement du pavé tactile | Zoomer et dézoomer |
| C | Recentrer la caméra derrière le véhicule |
| Échap | Quitter |

**Objectif** : atteindre le refuge en haut de la piste, ou explorer librement la forêt.

## Contenu

- Terrain procédural d'environ 120 × 165 m avec pente et chemin de terre.
- Arbres, rochers et cabane stylisés avec collisions Rapier.
- Châssis dynamique Rapier et 4 suspensions raycast à débattement court (36 cm).
- Roues visuelles animées et calées sur les points de contact au sol.
- Direction corrigée pour correspondre à la caméra arrière (D = droite à l'écran).
- Motricité, friction latérale, freinage et gamme courte simplifiées.
- Caméra orbitale à 360° : clic droit + souris pour inspecter le véhicule en roulant ou à l'arrêt.
- Zoom à la molette, vue arrière rétablie avec **C**, limites verticales et évitement simple des obstacles.
- Caméra stabilisée sur l'horizon, indicateur du nombre de roues au sol, HUD de vitesse et arrivée au refuge.

## Limites connues

Il s'agit d'un **prototype semi-arcade**, pas d'une simulation complète de pneus. Les roues ne sont pas des rigid bodies indépendants : le contact reste simulé par raycasts. Leur position visuelle suit toutefois la compression des suspensions. Pas de treuil, de boue déformable, de cargaisons ni de missions multiples. Le comportement de conduite reste à régler avec des tests de jeu.

## Vérification

```bash
cargo check
cargo test
```

Le dépôt inclut un workflow GitHub Actions pour vérifier le build et les tests sur Linux. Versions de départ : **Bevy 0.19.0**, **bevy_rapier3d 0.35.0**.

## Prochaines étapes

1. **Essayer la conduite sur PC** : vérifier que D tourne à droite de l'écran et que les pneus touchent visuellement le sol à l'arrêt.
2. Tester montée, descente, marche arrière, virages à différentes vitesses et franchissement du rocher.
3. Ajuster grip, raideur des ressorts et amortissement d'après le ressenti.
4. Ensuite seulement : ajouter un treuil et des missions de livraison.

### Vérification rapide après un pull

- Lancer `cargo run --release` puis attendre l'immobilisation du véhicule.
- L'affichage `Roues au sol: 4/4` doit apparaître sur une surface suffisamment plane.
- Avancer puis appuyer sur **D** : le véhicule doit tourner à droite dans l'image ; **Q/A** le fait tourner à gauche.
- Traverser une bosse : chaque roue doit suivre les variations du terrain, sans rebond excessif.
- Maintenir **clic droit** et déplacer la souris pour observer le véhicule de côté, de face et depuis le dessus ; la conduite reste active.
- Tester la **molette** pour le zoom, puis **C** pour revenir derrière le 4x4.
- Vérifier que la caméra ne traverse pas les arbres, les rochers ni les pentes ; l'évitement reste simple et pourra nécessiter des ajustements.
- Tester **L** pour la gamme courte ; utiliser **R** si le véhicule se retourne.

Le build et les tests automatiques ne remplacent **pas** ces essais graphiques et physiques en jeu.
