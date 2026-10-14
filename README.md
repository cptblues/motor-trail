# Motor Trail — prototype 4x4 Bevy + Rapier

Prototype **desktop** en Rust : une petite vallée low-poly procédurale, avec montée sinueuse, arbres, rochers, refuge et véhicule pilotable. Les décors sont générés à partir de primitives. **Le 4x4 utilise désormais le vrai modèle modulaire** fourni pour le projet, inclus dans `assets/vehicles/motor_trail_4x4.glb`.

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
| L | Basculer entre **L / courte** et **H / normale** sur le levier en bas à droite |
| R | Réinitialiser la position du véhicule |
| Clic droit maintenu + mouvement de souris | Tourner la caméra autour du véhicule (sens haut/bas corrigé) |
| Molette de souris / défilement du pavé tactile | Zoomer et dézoomer |
| C | Recentrer la caméra derrière le véhicule |
| Échap | Quitter |

**Objectif** : atteindre le refuge en haut de la piste, ou explorer librement la forêt.

## Contenu

- Terrain procédural d'environ 120 × 165 m avec pente et chemin de terre.
- Arbres, rochers et cabane stylisés avec collisions Rapier.
- Châssis dynamique Rapier et 4 suspensions raycast à débattement court (36 cm).
- Modèle low-poly `.glb` articulé : quatre pivots de suspension `Suspension_*`, pivots de direction `Steer_FL/FR` et rotations `WheelSpin_*`.
- Les roues visuelles du modèle suivent les raycasts de suspension, le braquage et la rotation.
- Direction corrigée pour correspondre à la caméra arrière (D = droite à l'écran).
- Motorisation plus vive, résistance au roulement réduite, vitesse normale relevée et gammes **courte / normale**.
- Suppression des efforts de braquage à très basse vitesse sans accélération pour limiter le pivotement à l'arrêt.
- Caméra orbitale à 360° : clic droit + souris pour inspecter le véhicule en roulant ou à l'arrêt.
- Zoom à la molette, vue arrière rétablie avec **C**, limites verticales et évitement simple des obstacles.
- Caméra stabilisée sur l'horizon, indicateur du nombre de roues au sol, **FPS**, vitesse et arrivée au refuge.
- Jauge visuelle en bas à droite, inspirée d'un levier de transfert (L / H), qui change avec la touche **L**.

## Limites connues

Il s'agit d'un **prototype semi-arcade**, pas d'une simulation complète de pneus. Les roues du modèle ne sont pas des rigid bodies indépendants : le contact reste simulé par raycasts. Leurs pivots visuels suivent toutefois la compression des suspensions, le braquage et le roulement. Pas de treuil, de boue déformable, de cargaisons ni de missions multiples. Le comportement de conduite reste à régler avec des tests de jeu.

## Vérification

```bash
cargo check
cargo test
```

Le dépôt inclut un workflow GitHub Actions pour vérifier le build et les tests sur Linux. Versions de départ : **Bevy 0.19.0**, **bevy_rapier3d 0.35.0**.

## Prochaines étapes

1. **Essayer la conduite sur PC** : vérifier que D tourne à droite, que les pneus touchent visuellement le sol et que les roues du nouveau modèle sont animées.
2. Tester montée, descente, marche arrière, virages à différentes vitesses et franchissement du rocher.
3. Ajuster grip, raideur des ressorts et amortissement d'après le ressenti.
4. Ensuite seulement : ajouter un treuil et des missions de livraison.

### Vérification rapide après un pull

- Lancer `cargo run --release` puis attendre l'immobilisation du véhicule.
- L'affichage `Roues au sol: 4/4` doit apparaître sur une surface suffisamment plane.
- Avancer puis appuyer sur **D** : le véhicule doit tourner à droite dans l'image ; **Q/A** le fait tourner à gauche.
- Traverser une bosse : chaque roue doit suivre les variations du terrain, sans rebond excessif.
- Maintenir **clic droit** et déplacer la souris pour observer le véhicule de côté, de face et depuis le dessus ; vérifier le nouveau sens vertical (souris vers le bas = caméra plus haute).
- Tester la **molette** pour le zoom, puis **C** pour revenir derrière le 4x4.
- Vérifier que la caméra ne traverse pas les arbres, les rochers ni les pentes ; l'évitement reste simple et pourra nécessiter des ajustements.
- Appuyer plusieurs fois sur **L** : le levier illustré doit basculer entre H (normale) et L (courte).
- Observer le compteur **FPS** lors d'une montée pour distinguer les limites du moteur physique d'une baisse de fluidité.
- À l'arrêt, tourner uniquement les roues à gauche/droite : la voiture ne doit pas pivoter sous l'effet de la direction.
- Vérifier la vitesse maximale sur la piste, puis essayer **R** si le véhicule se retourne.

Le build et les tests automatiques ne remplacent **pas** ces essais graphiques et physiques en jeu.


## Modèle 3D et reproductibilité

- Modèle source : `assets/vehicles/motor_trail_4x4.glb` (1,8 Mo).
- Script de génération d'origine : `assets/vehicles/generate_motor_trail_4x4.py`.
- Le modèle est orienté vers `-Z` ; son instance est tournée de 180° pour correspondre au `+Z` du contrôleur du jeu.
- Les pivots de suspension et de direction sont repérés par leurs noms glTF. Le véhicule fonctionne avec son collider Rapier simplifié.
- Un workflow dédié `Generate modular 4x4 model` permet de régénérer le GLB depuis le script source archivé.

Les tests CI vérifient la **compilation** et des propriétés simples du prototype ; la fluidité et le comportement routier doivent encore être évalués sur un ordinateur avec GPU.
