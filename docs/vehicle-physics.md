# Physique Modulaire Du 4x4

## Enveloppe De Carrosserie

`vehicle/collision.rs` definit des volumes locaux mesures sur le GLB reel,
apres la translation Y=-1 et le retournement graphique autour de Y.
Le collider racine est un compose : longerons, caisse, cabine convexe inclinee,
toit, capot, pare-chocs, treuil/protection, galerie, chargement, jerrican,
marchepieds, retroviseurs, snorkel et roue de secours fixe. Les quatre pneus
suspendus ne sont pas incorpores dans cette enveloppe.

L'ancien cuboid ne montait qu'a Y=0.40 m alors que le toit atteint 1.164 m et
le chargement 1.5225 m. Le test de chute sur le toit reproduisait 1.124 m de
penetration visuelle. Les nouveaux volumes protegent ces parties sans changer
le GLB, gonfler les pneus, teleporter ou redresser automatiquement le vehicule.
Un flanc peut naturellement basculer sur ses roues sous l'effet du centre de
masse bas ; le test controle le vrai contact et la continuite du mouvement.

Les petites pieces sont simplifiees, les surfaces minces ont une epaisseur
finie et la cabine n'est pas un interieur creux accessible. Le compose et ses
bounds sont caches ; ils ne sont pas reconstruits a chaque pas de 120 Hz.
Les crampons depassent encore de 2.5 cm environ la gomme de reference.

## Masse De Base Et Masse Effective

Le chassis reste **l'unique corps dynamique**. Le collider racine conserve les
proprietes explicites du profil : masse, centre de masse et approximation
analytique historique de l'inertie. Changer la geometrie de collision ne change
pas implicitement cette approximation. Sans equipement, les valeurs physiques
sont identiques a celles du profil ; aucune densite implicite n'est ajoutee.

Les equipements rigides portent `ColliderMassProperties::Density(0.0)` : leur
masse provient de leurs definitions, une seule fois dans l'agregat applique a
la racine. Parry compose les proprietes avec rotation du tenseur, centre de
masse pondere et theoreme des axes paralleles. Les inerties principales et leur
repere doivent etre fournis par l'auteur, pas deduits du mesh de rendu.

`VehiclePhysicsConfig` reste la base sauvegardable. `EquipmentRequest` contient
le catalogue, les fixations et le loadout demandes. `EquipmentRuntime` conserve
la derniere installation valide et sa revision. `VehiclePhysics.active_config`
porte les valeurs effectives de masse et de centre de masse pour la suspension
et les calculs existants. L'agregat complet d'inertie est applique au corps.
Une demande invalide conserve les equipements actifs et affiche une erreur.
Un changement de profil recalcule l'agregat sans modifier son fichier JSON.
Si la base est valide mais que sa combinaison avec les equipements ne l'est
pas, `R` et le retour en Conduite conservent la derniere configuration effective
acceptee. Ils effacent les vitesses et contacts, pas la masse/inertie active.

Un controle independant du tenseur complet detecte une mauvaise decomposition
des moments repetes dans la version epinglee de Parry/glamx. Les reperes
principaux deja connus permettent de recuperer les sommes symetriques sans
eigensolveur maison. Si aucun resultat ne reconstruit le tenseur a 2e-5 pres
de sa plus grande entree, l'installation est refusee explicitement plutot
qu'appliquee avec des axes d'inertie incorrects. Certains loadouts physiquement
valides mais non decomposables de facon fiable peuvent donc etre refuses.

## Fixations Versionnees

`configs/vehicles/motor_trail_4x4.mounts.json` est une definition distincte des
profils de conduite. Son nom a deux suffixes l'exclut de la liste des profils.
Le schema 1 contient `vehicle_id` et `mounts`. Chaque fixation definit :

- `id` stable et `category` sous forme de chaines extensibles.
- `local_pose` : translation en metres, quaternion XYZW, echelle unitaire.
- `max_mass_kg` optionnel et `allowed_equipment_types`.
- `occupied_zones` et `conflicting_zones` pour les incompatibilites spatiales.

| Fixation | Categorie | Position locale | Limite de conception |
| --- | --- | --- | --- |
| roof_main | roof | (0, 1.240, -0.420) | 150 kg |
| front_winch | front | (0, -0.087, 2.143) | 80 kg |
| rear_spare_wheel | rear | (0, 0.390, -2.070) | 100 kg |
| left_accessory | left_side | (-0.988, -0.099, -0.190) | 35 kg |
| right_accessory | right_side | (0.988, -0.099, -0.190) | 35 kg |

Les coordonnees viennent des supports du modele. Les limites de masse sont
des choix de conception explicites, pas des donnees mesurees dans le GLB.
Les fixations physiques ne dependent pas de la presence de noeuds graphiques.

## Definitions Et Installations

`EquipmentCatalog` (schema 1) contient des `definitions`. Une definition
`EquipmentDefinition` porte `id`, `type`, `glb_path` optionnel, `local_pose`
relative a la fixation, `mass_kg`, `local_center_of_mass`, `principal_inertia`,
`principal_inertia_local_frame`, `compatible_mount_categories`, `mode` et
`colliders`. Chaque collider a sa propre pose locale et une forme cuboid,
cylinder, ball ou capsule avec dimensions explicites. Les dimensions, poses,
identifiants, versions, masses et inerties sont valides avant installation.

`VehicleEquipmentLoadout` (schema 1) contient `vehicle_id` et `equipment`.
Chaque installation reference `instance_id`, `equipment_id` et `mount_id`.
L'occupation d'un support est exclusive. Les types ET categories doivent
correspondre ; les zones partagees ou incompatibles sont refusees, quel que
soit l'ordre des installations. Les references inconnues sont des erreurs.

- `rigid` : enfant du chassis, collider compose sans rigid body independant.
- `decorative` : sans collider, masse/inertie nulles, rendu seulement.
- `articulated` et `detachable` : valeurs reservees du schema, **refusees** a
  la validation actuelle. Ni joints ni transition de detachement ne sont livres.

Le prototype `--equipment-demo` installe un coffre cyan de 40 kg a
(0, 1.740, -0.420), de dimensions 0.60 x 0.40 x 0.80 m. Avec Baseline, la
masse devient 1840 kg et le centre de masse remonte selon la moyenne ponderee.
Sans GLB d'equipement, des boites de diagnostic representent les AABB locales
des formes ; seul le coffre cuboid a un rendu exactement identique a sa forme.
Un GLB fourni est charge comme enfant de l'equipement. Les anciens accessoires
du modele restent dans la carrosserie de base, sans remplacement automatique.
Il n'y a ni configurateur, ni editeur/sauvegarde de loadout dans l'interface.

## Filtrage Et Cycle De Vie

Rapier attache les colliders enfants au corps ancetre du chassis. Ils n'entrent
donc pas en collision entre eux et les requetes de pneus les excluent avec
`exclude_rigid_body(chassis)` et `exclude_sensors()`. Ce filtrage est commun au
rayon natif et au balayage volumetrique ; aucune couche globale ne masque les
obstacles, le sol ou les autres vehicules.

Les entites d'equipement appartiennent au vehicule, pas a la racine du terrain.
Recharger/generer une scene ne les supprime pas. Remplacer un loadout libere ses
entites et ses assets de diagnostic ; perdre le vehicule nettoie ces ressources.
La pose d'un enfant est chassis-local, collider et rendu compris : acceleration,
braquage et retournement ne requierent aucune recopie manuelle en coordonnees monde.

`R`, le retour en Conduite et le chargement calculent la garde au sol sur toute
l'empreinte de la carrosserie et des equipements rigides. Les hauteurs du terrain
et obstacles proches sont prises en compte. Une empreinte hors carte est
refusee explicitement. Le format JSON des scenes terrain n'est pas modifie.

## Contacts Des Pneus

Voir [Diagnostic des pneus](wheel-contact-diagnosis.md). Le mode natif delegue
au vrai controleur Rapier epingle. L'option `--experimental-tires`, egalement
disponible dans l'onglet Pneus, est une option de session desactivee par defaut.
Elle utilise un derive local isole seulement parce que le contact de roue
n'est pas injectable dans l'API actuelle. La licence et la provenance sont
conservees dans `third_party/rapier-LICENSE` et l'en-tete du module.

Le derive doit etre compare aux sources amont lors de chaque mise a jour
Rapier. La facade synchronise des caches prealloues, selectionne un seul backend
et appelle une seule fois les equations de forces. La limitation d'impulsion
combinee et le maintien en pente existants travaillent sur ce resultat unique.
Le profil Origine reste independant de ce choix.

## Diagnostic Et Verification

`V` ajoute aux roues : volumes de chassis/carrosserie/toit/pare-chocs/accessoires,
colliders d'equipements, axes locaux des fixations et centre de masse effectif.
Le panneau Diagnostic expose masse/centre de masse effectifs et backend actif.
L'affichage seul ne modifie pas la physique.

Le probe mesure separement les sommets de carrosserie et ceux des quatre
roues suspendues. Les pneus raycast n'ont pas de collision laterale complete :
l'essai natif a montre environ 11 cm d'intersection d'un moyeu lors du premier
contact de flanc, alors que la carrosserie physique etait a moins de 1 mm du sol.
Ce defaut est enregistre, pas masque par un changement de rayon ou de modele.
La roue de secours fixe reste incluse dans les mesures de carrosserie.

Les tests couvrent toit, flancs, pentes, rocher, pare-chocs avant/arriere,
mass/inertie, donnees versionnees et conflits, transformations, corps partage,
exclusion des requetes propres, application/retrait et conservation au rechargement.
Les probes natifs rendent le vrai GLB, sans modifier les profils utilisateur :

```powershell
cargo run --release -- --vehicle-body-probe "$env:TEMP\motor-trail-body"
cargo run --release -- --wheel-contact-probe "$env:TEMP\motor-trail-native"
cargo run --release -- --experimental-tires --wheel-contact-probe "$env:TEMP\motor-trail-envelope"
cargo run --release -- --terrain-smoke "$env:TEMP\motor-trail-terrain"
```

La sensation de conduite, les collisions a grande vitesse et les combinaisons
arbitraires de futurs accessoires demandent encore des essais manuels.

## Resultats Du 15 Octobre 2026

- `cargo fmt --all -- --check` : reussi.
- `cargo test` : 286 tests reussis, aucun ignore.
- `cargo test --release` : 286 tests reussis, aucun ignore.
- `cargo build --release` : reussi. Quatre avertissements `dead_code` concernent
  des points d'import/API verifies par les tests mais non utilises par le jeu courant.
- `git diff --check` : reussi (les notifications LF/CRLF de Git ne sont pas des erreurs).

Pour la Mission A seule, les profils physiques existants, Cargo.toml/Cargo.lock,
le GLB et l'animation des roues n'ont pas ete modifies. Les ajouts restent locaux sur la branche
de travail, sans nouveau commit, publication ni fusion dans cette mission.

Probes Vulkan/GLB rejoues sur le binaire final :

| Controle | Resultat |
| --- | --- |
| Carrosserie + coffre | Sortie 0, quatre captures, 1840 kg, suivi exact du coffre |
| Toit/flancs physiques | Penetration inferieure a 0.5 mm aux captures ; toit stabilise, flancs captures au contact reel |
| Pneus natifs | Sortie 0, quatre captures, 0.25109 m de penetration maximale, quatre roues sur le bloc |
| Pneus experimentaux | Sortie 0, quatre captures, 0.01863 m de penetration maximale, quatre roues sur le bloc |
| Atelier Terrain isole | Sortie 0, six captures, rotation 15 puis 10 deg, annulation, pose exacte, selection, JSON, Undo/Redo et 9.87 m de conduite |

Les captures de toit, de contact de flanc, de marche et d'apercu ont ete
inspectees. Les 18 images des passages reussis sont conservees dans les
repertoires locaux `mission-a-verified-*` des visualisations de cette tache.

Le passage Terrain groupe final s'est arrete sur "Preview validity disagrees
with the current ground pick", avec des erreurs Windows de positionnement
du curseur et une perte du peripherique audio dans le journal. La relance
isolee du meme binaire a reussi sans modification du code. La cause exacte
de cet echec intermittent n'est pas etablie ; il reste une limite du controle
graphique automatise, et ses journaux sont conserves. Les erreurs de curseur
Windows apparaissent aussi sur des passages reussis. Aucun ressenti de conduite
n'est presente comme valide automatiquement.

## Suivi De Fluidite

La correction suivante synchronise uniquement le rendu du chassis, des roues,
des equipements et de la camera, sans modifier les forces ni les appuis.
Voir [le diagnostic et les limites restantes](vehicle-presentation.md).
