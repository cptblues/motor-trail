# Mission A - Diagnostic Et Conception

## Etat De Depart

Branche inspectee : `codex/terrain-workshop`, commit `8c04082`.
Bevy 0.19.0, bevy_rapier3d 0.35.0, Rapier 0.33.0-alpha, bevy_egui 0.42.0,
Rust edition 2024, simulation a 120 Hz. Le depot contient le diagnostic et
l'apercu de placement, sans adaptateur de forces local. Le choix precedent
de garder le controleur natif reste la reference de cette mission.

Ce document conserve le diagnostic A1 et la conception acceptee avant les edits.
L'implementation est decrite dans [Physique modulaire](vehicle-physics.md) ;
les resultats finaux sont consignes dans la section de validation ci-dessous.

## A1 - Diagnostic Verifie

`chassis_bundle` contient un seul `Collider::cuboid(1.10, 0.40, 1.92)`, centre
sur le corps dynamique. Il s'etend donc de -0.40 a +0.40 m verticalement.
Le GLB est enfant de ce corps avec translation (0, -1, 0) et rotation PI autour
de Y. Il n'y a pas de collider de cabine, de toit ou de chargement.

Les bornes suivantes sont lues dans les accessors POSITION du GLB reel, puis
exprimees dans le repere du corps. Les groupes sans transformation locale
herite sont aussi compares au generateur pour identifier les sous-volumes.

| Groupe | Bornes Y locales au corps | Autres mesures utiles |
| --- | --- | --- |
| Chassis | -0.42277 a 0.01600 m | X +/-0.85 m, Z +/-1.955 m |
| Body | -0.04000 a 1.16400 m | X +/-0.87 m, Z -1.80 a 1.84 m |
| Windows | 0.63813 a 1.09192 m | X +/-0.768 m |
| Details | -0.47541 a 1.26750 m | retroviseurs X +/-1.14 m |
| Bumpers_Recovery | -0.21234 a 0.61448 m | Z -2.1525 a 2.3440 m |
| Roof_Rack_Equipment | 1.16750 a 1.52250 m | X +/-0.775 m, Z -1.386 a 0.516 m |

La roue de secours est un groupe transforme separement : centre local au corps
(0, 0.39, -2.07), rayon maximal proche de 0.555 m et demi-epaisseur 0.239 m.
Les roues suspendues ne doivent pas etre incluses dans un grand volume de cabine.

### Cause Du Retournement

Sur le toit, le collider actuel peut reposer avec le pivot environ 0.40 m
au-dessus du sol. Le sommet du toit visuel se trouve alors environ 0.764 m sous
le sol, et le chargement peut descendre environ 1.123 m sous le sol. Ce sont les
volumes manquants de collision, pas une correction automatique de rotation.
Sur un flanc incline, l'absence de la partie haute produit le meme type d'ecart.
Les rayons des suspensions tournent avec le chassis : sur le toit, ils pointent
vers le haut et ne peuvent pas remplacer une enveloppe de collision de cabine.

Les remises en position sont volontaires (R) ou liees au chargement d'une scene
et au retour en mode Conduite. Aucun redressement automatique apres retournement
n'a ete trouve dans les systemes inspectes.

### Masse Et Requetes Actuelles

Le collider racine porte `ColliderMassProperties::MassProperties` : masse et
centre de masse viennent du profil ; l'inertie utilise l'approximation analytique
du cuboid historique. Il ne faut pas ajouter des colliders de densite implicite
qui additionneraient leur masse a ce reglage explicite.

Les roues natives excluent le corps du vehicule et les capteurs de leurs
requetes. Les colliders d'equipements devront appartenir a ce meme corps pour
beneficier de cette exclusion, plutot qu'a de nouveaux corps dynamiques.

Le spawn verifie une empreinte fixe de +/-1.3 m en X et +/-2.2 m en Z. Cette
profondeur ne couvre pas le treuil avant (+2.344 m) ni toute la roue de secours.
La remise en position devra calculer l'empreinte de l'enveloppe complete et des
equipements, sans modifier la hauteur normale pour masquer le retournement.

## Conception Proposee

### A2 - Carrosserie

Preferer un collider compose racine, forme de volumes simples locaux : chassis
inferieur, caisse, cabine avec montants inclines, toit, capot, pare-chocs,
protection avant, galerie/chargement deja presents et roue de secours fixe.
Les details fins sont simplifies et leur tolerance documentee. Aucune roue
suspendue n'est incluse dans cette enveloppe.

Alternative : un collider enfant par volume avec masse nulle. Elle facilite
le diagnostic par piece, mais multiplie les colliders a synchroniser et exige
plus de precautions de masse. Un grand cuboid unique engloberait du vide et
les roues ; il ne convient pas au franchissement.

Dans les deux cas : un seul corps dynamique, proprietes de masse explicites
du profil, pas de changement implicite de l'inertie sans equipement. Le shape
compose sert aux collisions ; sa densite geometrique ne pilote pas la masse.

### A3 - Pneus

Conserver le controleur natif par defaut et le diagnostic existant. La requete
volumetrique de `contacts.rs` est une comparaison geometrique sans forces.
Son integration au solveur demande un remplacement du contact avant les calculs
de charge/amortissement. L'API epinglee n'expose pas ce point d'injection.

Ne pas reintroduire une copie du controleur comme comportement par defaut.
L'utilisateur a accepte un mode experimental, isole et desactive par defaut,
compare au natif et documente comme tel. Pas de colliders
de roues supplementaires ni d'impulsion arbitraire pour aider a grimper.

### A4 - Fixations Et Equipements

Definir des identifiants stables et categories extensibles sous forme de
chaines validees, des transformations locales serialisables et une definition
de vehicule versionnee, independante des entites et noms de noeuds GLB.

Reperes de reference a verifier avec les sous-volumes du modele :

| Identifiant | Categorie | Position locale mesuree | Reference |
| --- | --- | --- | --- |
| roof_main | roof | (0, 1.240, -0.420) | base de galerie |
| front_winch | front | (0, -0.087, 2.143) | support de treuil |
| rear_spare_wheel | rear | (0, 0.390, -2.070) | pivot de roue de secours |
| left_accessory | left_side | (-0.988, -0.099, -0.190) | face externe du marchepied |
| right_accessory | right_side | (0.988, -0.099, -0.190) | face externe du marchepied |

Les masses maximales seront des limites de conception explicites, pas des
mesures pretendument extraites du GLB. Definir compatibilite, occupation et
conflits de zones ; refuser les references inconnues et les transformations
non finies. Les accessoires deja visibles font partie du vehicule de base :
leur remplacement visuel reste hors de cette mission.

Un equipement rigide est enfant du vehicule, avec collider sans masse implicite
et sans nouveau rigid body. Une definition decorative n'a pas de collider.
Articulations et detachement peuvent etre representes mais doivent etre refuses
comme operations non supportees, pas presentes comme fonctionnels.

Separer profil de base, loadout et proprietes effectives. Utiliser les
`MassProperties` de Rapier/Parry pour rotation, composition et axes paralleles,
plutot qu'une addition de masses seule. Sans equipement, l'identite avec le
profil doit etre exacte. L'application de la masse d'equipement peut commencer
par un prototype explicite, avec tests de centre de masse et d'inertie.

### Diagnostics Et Integration Terrain

Etendre V : carrosserie, equipements, fixations et centre de masse de couleurs
distinctes, sans effet sur les forces. Les equipements restent attaches au
vehicule lors des regenerations ; le terrain ne doit pas posseder ces entites.
La remise en position doit utiliser les bornes completes de collision.

## Validation Prevue

- Reproduire le toit sans protection avant d'activer les nouveaux volumes.
- Stabiliser sur roues, deux flancs et toit ; mesurer l'enveloppe apres contact.
- Tester plan incline, relief, rocher, collision avant/arriere et glissement.
- Verifier masse, centre de masse et inertie, sans double comptabilisation.
- Tester exclusion des colliders propres des rayons et requetes volumetriques.
- Tester transformations des fixations apres roulis/retournement et JSON.
- Tester equipement factice, occupation, incompatibilite et conflits.
- Rejouer les regressions des deux ateliers et les verifications Cargo.

Le ressenti de conduite reste une validation manuelle distincte.

## Verification De La Base

Apres retrait de l'adaptateur : 201 tests normaux et 201 tests release reussis,
sans test ignore ; formatage et compilation release reussis. controller.rs,
visuals.rs et le module d'apercu n'ont pas ete modifies pendant A1.
Ces chiffres concernent uniquement la base A1. Les tests natifs caracterisent
le defaut raycast ; les tests experimentaux distincts evaluent le contact nouveau.

## Increments Verifies De Mission A

- Rouge/vert toit : 1.1239514 m de penetration de chargement avant le compose,
  puis enveloppe protegee apres sa mise en place, sans modification du GLB.
- Donnees : tests d'abord en echec pour les validations et calculs manquants,
  puis 9 tests de fixations et 18 tests d'equipements reussis.
- Backend : test d'appui sur arete d'abord en echec avec le natif seul,
  puis 13 tests de facade/parite et de contacts experimentaux reussis.
- Marche 25 cm headless : penetration maximale de gomme 0.25315 m en avant et
  0.24988 m en arriere en natif ; 0.03741 m et 0.03320 m en experimental.
- Requetes repetees avec plafond rejete : 448 allocations pour 32 requetes
  avant correction, puis zero avec tampon fixe et sans geometrie EPA inutile.
- R et telemetrie : tests d'abord en echec sur l'encombrement d'equipement et
  les valeurs effectives absentes ; raccordement realise ensuite.

Les suites finales (286 tests normaux et 286 release), probes natifs et limites
de validation sont consignes dans la documentation de physique modulaire.
Elle inclut la penetration laterale residuelle des roues suspendues et l'echec
intermittent d'un passage Terrain, suivi d'une relance isolee reussie.
