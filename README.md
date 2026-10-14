# Motor Trail

Prototype Rust d'exploration 4x4 dans une vallee low-poly, avec Bevy 0.19,
bevy_rapier3d 0.35, le modele articule fourni et un atelier de physique en direct.

## Demarrer

Installer un Rust stable a jour, puis lancer :

```powershell
rustup update stable
cargo run --release
```

La premiere compilation du moteur peut prendre plusieurs minutes.

## Commandes

| Touche | Action |
| --- | --- |
| Z / W / fleche haut | Accelerer |
| S / fleche bas | Freiner puis reculer |
| Q / A / fleche gauche | Braquer a gauche |
| D / fleche droite | Braquer a droite |
| Espace | Frein prioritaire sur l'accelerateur |
| L | Basculer gamme normale / courte |
| R | Repositionner le 4x4 sans perdre le profil courant |
| Clic droit + souris | Tourner la camera |
| Molette | Zoom |
| C | Recentrer la camera |
| `²` | Ouvrir / fermer l'atelier |
| Echap | Quitter |

Le raccourci de l'atelier reconnait le caractere `²` et la position physique
`KeyCode::Backquote`, a gauche de la touche 1 sur un clavier AZERTY courant.
Il fonctionne aussi pendant l'edition d'une valeur, sans touche de fonction.

## Atelier

La fenetre est deplacable et redimensionnable. Les onglets regroupent :

| Onglet | Reglages / mesures |
| --- | --- |
| Chassis | Masse (kg), centre de masse (m), amortissements et aide au maintien en pente |
| Suspensions | Avant / arriere : k (N/m), compression / detente (N*s/m), repos, debattement (m) et force maximale par roue (N) |
| Pneus | Rayon (m), limite d'adherence combinee, reponse laterale, resistance au roulement (N) et trainee (N*s/m) |
| Moteur | Force totale (N), reponse, rapports, vitesses cibles, frein de service et frein moteur |
| Diagnostic | FPS, frame, vitesse, acceleration, lacet, angles, contacts, compression et charge des quatre roues |

Les valeurs validees sont appliquees au prochain pas de physique. Une valeur
invalide affiche une erreur et laisse la configuration active intacte.
Un changement de masse ou d'amortissement reveille le chassis. Les changements
de geometrie reinitialisent les contacts et l'historique visuel de roulement,
sans teleporter le vehicule. `R` permet de repartir dans une position stable.

La souris utilisee dans l'atelier ne fait pas tourner la camera. Le clavier
reste disponible pour conduire lorsque le panneau n'edite pas de texte ou de
valeur et n'a pas de menu ouvert.

Les marqueurs signalent les differences avec Baseline. La comparaison A/B
montre les valeurs actives face a un profil de reference ; appliquer B copie
ses valeurs sans modifier le fichier sauvegarde.

## Profils

Les JSON lisibles sont dans `configs/vehicles/`.

- **Baseline** : reference du controleur Rapier.
- **Confort**, **Franchissement**, **Dynamique** : variantes experimentales.
- **Origine** : coefficients historiques et controleur maison pour comparaison.

Ces profils ne sont pas des reglages optimaux. Sauvegarder avec deux noms
distincts permet de comparer des essais puis de les recharger. Le dernier
profil charge ou sauvegarde est restaure au demarrage. Un fichier invalide
produit une erreur visible et un repli sur la reference integree, sans planter.
Les fichiers de reference existants ne sont pas remplaces au demarrage.
Le bouton de retour aux valeurs initiales restaure la Baseline integree.

## Modele Physique

Le [controleur raycast de Rapier, issu de Bullet](https://rapier.rs/docs/user_guides/python/vehicle_controller/)
gere ressorts, amortisseurs, traction, freinage et friction limitee par la
charge. Il est accessible dans la version Rapier embarquee par le plugin.
Les calculs tournent a **120 Hz**, apres la synchronisation du backend et avant
son integration, selon le [mode a pas fixe du plugin](https://rapier.rs/docs/user_guides/bevy_plugin/integration_parameters/).
Un seul controleur applique des forces a chaque pas.

La version [Rapier 0.33-alpha embarquee](https://docs.rs/crate/rapier3d/0.33.0-alpha/source/src/control/ray_cast_vehicle_controller.rs)
peut omettre sa limite longitudinale en ligne droite. L'adaptateur retire
l'exces de son impulsion et impose un cercle de friction `mu * charge * dt`
sur la charge effectivement plafonnee. Le maintien en pente partage ce meme
budget entre les contacts ; une faible adherence peut donc laisser glisser
le vehicule, meme avec l'aide active.

Les coefficients physiques de ressort et d'amortisseur sont convertis en
coefficients Rapier par division par la masse du chassis : l'API les multiplie
elle-meme par cette masse. Le frein de service en N est converti en impulsion
par roue : `force_totale * dt / 4`.

Le profil Origine conserve son frein historique proportionnel a la vitesse,
avec un coefficient distinct en **N*s/m**. Son aide historique au lacet reste
limitee a ce modele de comparaison. Le controleur Rapier n'ajoute pas ce couple.

L'aide de maintien, explicitement reglable de 0 a 1, agit uniquement presque
a l'arret, avec au moins trois contacts et sur une pente moderee. Elle laisse
les suspensions travailler, y compris quand le frein prend priorite sur
l'accelerateur. Le vehicule reste simule sans endormissement automatique pour
ne pas masquer les glissements lents. Un corps endormi explicitement ne recoit
pas d'impulsions qui creeraient des vitesses fictives.
La compensation tient compte des sous-pas internes de Rapier et reprend les
memes points d'application que son controleur natif.

Les quatre points de contact sont alignes sur les moyeux du GLB. Chaque roue
suit sa suspension et son deplacement tangent au sol **apres** simulation,
avec le signe corrige pour le modele retourne de 180 degres. Les roues ont des
rotations individuelles en virage et ne tournent pas du seul fait d'une vitesse
de compensation ou d'un mouvement du chassis dans les airs.

L'interface utilise [bevy_egui 0.42, compatible Bevy 0.19](https://docs.rs/crate/bevy_egui/0.42.0).

## Mesures Et Limites

- La charge affichee avec Rapier est sa force de suspension plafonnee appliquee.
  Avec Origine, la charge est estimee par le ressort-amortisseur historique.
- L'acceleration longitudinale est une derivee numerique de la vitesse.
  Le glissement affiche est une estimation de derive laterale, en m/s.
- Les roues raycast n'ont pas d'inertie de rotation independante : le rapport de
  glissement longitudinal d'un vrai pneu n'est pas mesure.
- La limite de friction Rapier est combinee ; la reponse laterale est un
  multiplicateur du solveur, pas un second coefficient de friction applique.
- Les diagnostics moteur / frein indiquent les demandes ou limites disponibles.
- Le controleur Rapier garde une influence interne de roulis non exposee par
  cette version de son API. Il n'y a pas de barre antiroulis supplementaire.
- `SurfaceGrip` permet d'associer ulterieurement un multiplicateur a un collider ;
  les surfaces actuelles utilisent le facteur par defaut.
- Les limites de validation de charge statique supposent une repartition egale
  entre roues. Les transferts reels se lisent dans le diagnostic.

Le ressenti et les reglages doivent etre essayes en jeu. Des tests et une
compilation reussie ne constituent pas une validation des sensations de conduite.

## Architecture

```text
src/
  main.rs                    vallee, modele, camera et commandes du jeu
  vehicle/config.rs          parametres, unites, validation et references
  vehicle/controller.rs      Rapier / Origine et application en direct
  vehicle/visuals.rs          suspensions, direction et roulement du GLB
  vehicle/telemetry.rs        mesures et estimations
  ui/physics_tuning.rs        panneau, comparaison et capture des entrees
  persistence/profiles.rs    JSON et dernier profil selectionne
```

## Verifier

```powershell
cargo test
cargo test --release
cargo build --release
git diff --check
```

Les tests couvrent validation, profils, raccourci, rendu egui multipasse,
pentes, terrain reel, marche arriere, freinage et commandes a l'arret.
Le profil de test utilise des symboles de diagnostic limites pour contenir le
cout d'edition des liens sous Windows. Les assertions debug et controles de
depassement restent actifs ; le profil release du jeu est inchange.
Pour les essais manuels, comparer les memes passages avec deux profils :
arret en devers, demarrage en pente, freinage, virage et une roue sur un obstacle.
Regler d'abord la charge statique et l'amortissement, puis adherence et moteur.
