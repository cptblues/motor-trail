# Motor Trail

Prototype Rust d'exploration 4x4 dans une vallee low-poly, avec Bevy 0.19,
bevy_rapier3d 0.35, le modele articule fourni et un atelier de physique en direct.

## Telecharger Motor-Trail

Les applications seront disponibles dans les [GitHub Releases](https://github.com/cptblues/motor-trail/releases)
apres publication d'un tag de version :

- Windows x64 : extraire **tout** `Motor-Trail-Windows-x64.zip`, puis ouvrir
  `Motor-Trail/motor-trail.exe` par double-clic.
- Mac Apple Silicon (M1 et suivants) : extraire `Motor-Trail-macOS-AppleSilicon.zip`,
  puis ouvrir `Motor-Trail.app`, eventuellement deplacee dans Applications.
- Mac Intel : utiliser `Motor-Trail-macOS-Intel.zip` de la meme facon.

Rust et Cargo ne sont pas necessaires pour jouer. Les applications ne sont pas
signees ni notarisees : Windows ou Gatekeeper sur macOS peuvent afficher un
avertissement ou bloquer le lancement. Ne pas desactiver les protections du
systeme ; consulter les indications officielles et verifier la provenance.
L'icone generique du systeme est utilisee temporairement.

Les sauvegardes sont stockees hors de l'installation :
`%APPDATA%/MotorTrail/` sous Windows et
`~/Library/Application Support/MotorTrail/` sous macOS.
Remplacer l'application ne remplace pas ces donnees. Voir
[distribution et validation](docs/distribution.md) pour la migration et les essais manuels.

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
| V | Diagnostic 3D : pneus, carrosserie, equipements, fixations et centre de masse |
| Echap | Quitter |

Le raccourci de l'atelier reconnait le caractere `²` et la position physique
`KeyCode::Backquote`, a gauche de la touche 1 sur un clavier AZERTY courant.
Il fonctionne aussi pendant l'edition d'une valeur, sans touche de fonction.

## Atelier Terrain

`T` ouvre l'atelier Terrain hors saisie de texte ; `²` reste reserve aux
reglages physiques. Passer explicitement en **Edition** pour generer, sculpter
ou placer des objets, puis en **Conduite** pour essayer le resultat.

L'atelier propose des terrains seedes (Plat, Vallee, Montagne, Trial), quatre
brosses, cinq primitives, selection 3D et transformations numeriques, Undo/Redo,
huit scenes de reference et des sauvegardes JSON exactes dans le sous-dossier
`terrains/` des donnees utilisateur.
La vallee historique reste chargeable. Les profils du 4x4 sont conserves.

En edition : clic gauche pour l'outil, clic droit pour l'orbite, clic molette
pour deplacer la camera et molette pour zoomer. Ctrl+Z annule ; Ctrl+Y ou
Ctrl+Maj+Z retablit. Le vehicule est immobilise puis replace au depart lors du
retour en conduite, apres synchronisation des nouvelles collisions.

### Apercu de placement

L'outil **Placer** affiche la geometrie finale en transparence, sans collision :
vert si la pose est valide, rouge hors terrain ou avec des dimensions invalides.
L'apercu suit la souris et s'aligne sur le sol. Q tourne de -15 deg, E de +15 deg ;
avec Maj, le pas est de 5 deg. Le lacet est conserve par primitive entre les poses.
Les positions physiques Q/A sont acceptees pour les claviers QWERTY et AZERTY.
Clic gauche confirme la pose affichee ; les clics dans l'interface ne posent rien.
Clic droit ou Echap annule le placement et repasse en selection. Pendant le
placement, Echap ne quitte pas le jeu et Q/E ne commandent pas le vehicule.

Voir [Terrain Workshop](docs/terrain-workshop.md) pour les commandes, l'architecture,
le format JSON, les limites de reconstruction et les extensions vers les chunks.

## Atelier Physique

La fenetre est deplacable et redimensionnable. Les onglets regroupent :

| Onglet | Reglages / mesures |
| --- | --- |
| Chassis | Masse (kg), centre de masse (m), amortissements et aide au maintien en pente |
| Suspensions | Avant / arriere : k (N/m), compression / detente (N*s/m), repos, debattement (m) et force maximale par roue (N) |
| Pneus | Rayon (m), adherence, reponse laterale, resistance, trainee et contacts volumetriques experimentaux |
| Moteur | Force totale (N), reponse, rapports, vitesses cibles, frein de service et frein moteur |
| Diagnostic | Masse/centre de masse effectifs, backend pneus, FPS, vitesse, angles, contacts, compression et charge des roues |

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

Les JSON modifiables sont dans `vehicles/` des donnees utilisateur. Les
references livrees se trouvent dans `defaults/vehicles/` des applications, ou
dans `configs/vehicles/` du depot en developpement. La premiere migration copie
les anciens profils et terrains sans les supprimer ni ecraser les sauvegardes.

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
Un seul controleur applique des forces a chaque pas. Le controleur Rapier natif
reste intact et actif par defaut. Une option de session explicitement
experimentale permet de comparer un contact volumetrique, sans doubler les forces.

### Carrosserie Et Equipements

Un collider compose couvre le chassis, la cabine, le toit, les pare-chocs,
les accessoires fixes et la roue de secours mesures sur le GLB. Il reste un
seul corps dynamique, avec la masse et l'inertie du profil de base conservees.
Aucune correction de redressement automatique n'est ajoutee.

Les fixations versionnees dans `configs/vehicles/motor_trail_4x4.mounts.json`
permettent des equipements rigides ou decoratifs valides, sans interface de
personnalisation. Le prototype `cargo run --release -- --equipment-demo` ajoute
un coffre factice de 40 kg sur le toit : sa masse, son centre de masse et son
inertie sont combines sans modifier le profil sauvegarde. `R`, le retour en
Conduite et le rechargement tiennent compte de son encombrement. Un depart dont
l'enveloppe sort du terrain est refuse avec un message.

Voir [Physique modulaire](docs/vehicle-physics.md) pour les schemas, filtres,
limites et modes articules/detachables reserves a une version future.

### Pneus Experimentaux

Dans **Pneus**, cocher **Contacts volumetriques (experimental)**, ou lancer :

```powershell
cargo run --release -- --experimental-tires
```

Cette option est **desactivee par defaut** et n'est pas enregistree dans les
profils JSON. Elle remplace le contact choisi avant l'unique calcul de suspension
et de friction. Elle ne change ni le rayon, ni la position du modele.
Le profil Origine garde son controleur historique independant.
Les tests de marche de 25 cm mesurent environ 3-4 cm de penetration de gomme
en mode experimental, contre environ 25 cm avec le rayon natif. Ce n'est pas
une simulation de pneu deformable ni une garantie contre les chocs rapides.
Les roues suspendues n'ont pas de collision laterale complete, notamment
pendant un basculement sur le flanc ; cette limite reste documentee.

### Diagnostic 3D des pneus

`V` affiche les centres physiques (blanc), le rayon et la largeur de gomme (cyan),
les rayons descendants (rouge), les contacts utilises (orange) et les colliders
des obstacles. Les marques vert clair montrent les contacts candidats du
balayage volumetrique en lecture seule. L'affichage n'applique aucune force ;
seule l'option experimentale distincte change les contacts utilises par le controleur.
Les centres restent visibles a travers le modele dans ce mode de diagnostic.
La touche est inactive pendant une saisie dans l'interface ; aucun profil n'est
modifie. On peut aussi demarrer avec `cargo run --release -- --wheel-diagnostics`.
Voir [Diagnostic des contacts](docs/wheel-contact-diagnosis.md) pour les mesures
du GLB et la comparaison des deux methodes. Le defaut de penetration des pneus
reste present dans le mode natif de reference.

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
  app_paths.rs               ressources d'installation et donnees utilisateur
  app_paths/                 migration initiale et controle sans interface
  main.rs                    modele, camera de conduite et commandes du jeu
  terrain/                   generation, sculpture, scenes, objets et collisions
  vehicle/config.rs          parametres, unites, validation et references
  vehicle/controller.rs      Rapier / Origine et application en direct
  vehicle/collision.rs       enveloppe composee mesuree sur le GLB
  vehicle/contacts.rs        requetes volumetriques partagees, sans forces
  vehicle/wheel_backend.rs   natif intact ou essai volumetrique isole
  vehicle/mounting.rs        fixations locales versionnees
  vehicle/equipment.rs       schemas, compatibilite et masses combinees
  vehicle/equipment_runtime.rs  attachements rigides et application transactionnelle
  vehicle/diagnostics.rs     centres, contacts, carrosserie et fixations (V)
  vehicle/visuals.rs          suspensions, direction et roulement du GLB
  vehicle/presentation.rs     interpolation graphique et pose suivie par la camera
  vehicle/telemetry.rs        mesures et estimations
  ui/physics_tuning.rs        panneau, comparaison et capture des entrees
  ui/terrain_workshop.rs      atelier terrain et commandes d'edition
  ui/input_capture.rs         capture commune aux deux panneaux
  persistence/profiles.rs    JSON et dernier profil selectionne
```

## Verifier

Le rendu du 4x4, des roues et des equipements est interpole entre les pas
physiques a 120 Hz ; la camera suit le meme instant. Les colliders et les
calculs de forces ne sont pas lisses. Voir
[le diagnostic de fluidite et les limites des roues coincees](docs/vehicle-presentation.md).

```powershell
cargo fmt --all -- --check
cargo test
cargo test --release
cargo build --release
python -B -m unittest discover -s scripts/tests -v
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

## Creer Une Release

Integrer d'abord les changements dans `main`, verifier la CI et utiliser un tag
de version encore inexistant, cree sur le commit integre et teste :

```bash
git checkout main
git pull
git tag v0.1.0-alpha.1
git push origin v0.1.0-alpha.1
```

Seul un push de tag `v*` declenche `.github/workflows/release.yml`. Un commit sur
`main`, une PR ou une autre branche ne produit pas d'application distribuable,
et aucun declenchement manuel n'est prevu. La CI Rust existante reste independante.

Les trois compilations natives doivent reussir, avec des archives valides et
un controle sans interface de l'installation extraite. La Release publie alors :

- `Motor-Trail-Windows-x64.zip`
- `Motor-Trail-macOS-AppleSilicon.zip`
- `Motor-Trail-macOS-Intel.zip`

Les tags contenant `alpha`, `beta` ou `rc` produisent une prerelease. Les details,
limites de validation et commandes locales sont dans [distribution](docs/distribution.md).
