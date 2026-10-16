# Terrain Workshop

## Utilisation

```powershell
cargo run --release
```

Le jeu demarre dans la vallee historique, en conduite. `T` ouvre ou ferme
l'atelier Terrain, sauf pendant une saisie de texte. Le panneau physique garde
son raccourci `²` / Backquote. Les deux fenetres peuvent rester ouvertes.

| Entree | Edition | Conduite |
| --- | --- | --- |
| Boutons Edition / Conduite | Choix explicite du mode | Choix explicite du mode |
| Clic gauche + glisser | Brosse active | Aucun outil terrain |
| Clic gauche, outil Placer | Placer la primitive au sol | Aucun placement |
| Clic gauche, outil Selectionner | Selection 3D de l'objet | Aucune selection |
| Q / E, outil Placer | Rotation de -15 / +15 deg (Q/A selon disposition) | Commandes de conduite normales |
| Maj + Q / E, outil Placer | Rotation fine de -5 / +5 deg | Commandes de conduite normales |
| Clic droit ou Echap, outil Placer | Annuler le placement, sans quitter | Echap quitte le jeu |
| Clic droit + glisser | Orbite autour de la zone d'edition | Camera du 4x4 |
| Clic molette + glisser | Translation du point de vue | Inactif |
| Molette | Zoom de l'editeur | Zoom du 4x4 |
| Ctrl+Z | Annuler | Reserve a la saisie de texte |
| Ctrl+Y / Ctrl+Maj+Z | Retablir | Reserve a la saisie de texte |
| Suppr | Supprimer l'objet selectionne | Inactif |
| R | Inactif | Replacer au depart de la scene courante |

Les commandes de conduite et profils physiques existants sont conserves.
En edition le chassis est un corps fixe, sans commande moteur. Repasser en
conduite replace le 4x4 au depart, remet a zero vitesses, forces, contacts et
historique des roues, puis attend la synchronisation physique avant de reprendre.
Le profil du vehicule n'est pas modifie.
Le depart est valide sur toute l'emprise du chassis, y compris apres rotation.
Le repositionnement tient compte des sommets du relief et des objets voisins
pour eviter une penetration au premier pas de simulation.

## Panneaux

- **Generation** : Plat, Vallee, Montagne, Trial ; seed u64, dimensions,
  resolution maximale par cellule, amplitude, frequence, octaves, persistance,
  lacunarite et rugosite. La vallee historique conserve sa grille d'origine.
- **Sculpture** : Elever, Abaisser, Lisser, Aplanir ; rayon, force/vitesse,
  exposant de retombee, altitude cible et prelevement au curseur.
- **Objets** : Cube, Cylindre, Sphere, Rocher, Rampe ; placement et selection
  dans la vue 3D, position/rotation/dimensions numeriques, appliquer, aligner,
  dupliquer et supprimer. La rampe monte suivant son axe local +Z.
- **Scenes** : nouvelle scene, references reproductibles, fichiers utilisateur,
  sauvegarder, sauvegarder sous, charger, position et orientation du depart.
  Les remplacements de scene et de fichiers demandent confirmation.
- **Diagnostic** : compteurs, resolution, etat de synchronisation, duree de
  generation, memoire canonique approximative et historique. Les erreurs restent
  visibles sans modifier la scene active lorsqu'un chargement echoue.

Une operation de brosse se termine au relachement du clic, a la perte de focus
ou lors d'une action de l'atelier. Un glisser demarre sur egui reste attribue a
l'interface jusqu'au relachement. Un changement de camera interrompt la brosse.

L'outil Placer utilise un vrai mesh transparent, vert pour une pose valide et
rouge si le rayon manque le terrain ou si les dimensions sont invalides. Une
pose valide ne signifie pas absence de recouvrement avec d'autres objets : les
assemblages d'obstacles statiques restent possibles. Le lacet est conserve par
primitive lors des placements repetes. L'annulation repasse en Selectionner.

## References

Les scenes integrees sont : Vallee historique, Terrain plat, Montee et descente,
Devers, Croisement de ponts, Marches successives, Rochers et Ornieres.
Les obstacles techniques et ornieres ont des dimensions explicites, pas un
tirage aleatoire. Les references sont immuables sur disque : enregistrer une
variante sous un nouveau nom. `configs/terrains/Exemple Plat.json` est un petit
document complet avec un rocher et une rampe, destine a illustrer le format.
`Exemple Relief.json` montre un snapshot sculpte qui differe du generateur
(amplitude nulle, mais sommet central releve). Ces deux exemples de 8 m sont
volontairement petits ; les parcours integres font normalement 64 x 64 m.

## Architecture

```text
src/terrain/
  config.rs       bornes, presets et dimensions
  data.rs         grille canonique et interpolation par triangle
  generation.rs   bruit Perlin multicouche, algorithme versionne
  mesh.rs         positions, indices, normales et trimesh Rapier communs
  brush.rs        modifications locales, instantane de lissage et deltas
  obstacles.rs    dimensions bakees, meshes et colliders des primitives
  scene.rs        document de scene, references et depart
  history.rs      operations reversibles et budget memoire
  persistence.rs  JSON valide, lectures bornees et publication atomique
  legacy.rs       relief, piste, arbres, rochers et refuge historiques
  runtime.rs      proprietaire de scene, actions, camera, picking et physique
src/ui/
  input_capture.rs      capture commune aux deux ateliers
  terrain_workshop.rs   fenetre egui et actions uniques par frame
```

`runtime/placement_preview.rs` possede uniquement l'entite et les assets
temporaires d'apercu. La fabrique `Obstacle::build_mesh` ne construit aucune
forme physique. La confirmation utilise la meme pose candidate fraiche puis
`commit_objects` cree l'objet persistant et son collider. Annuler, changer
d'outil, passer en conduite ou decharger une scene supprime l'apercu et ses
assets. Aucun de ces elements temporaires n'entre dans le document JSON.

`TerrainData` est une heightmap centree sur l'origine, independante de l'affichage.
Les cellules sont `ceil(dimension / resolution)` ; l'espacement effectif est
`dimension / cellules`. Les sommets comprennent les deux bords de la grille.
L'ordre des triangles est `[a,c,b]`, `[b,c,d]`, orientes vers +Y. La consultation
de hauteur interpole le triangle correspondant, pas une surface bilineaire
qui differerait du collider. Les normales de consultation sont geometriques ;
les normales du mesh sont moyennes par sommet pour l'eclairage.

La seule nouvelle dependance est [noise 0.9.0](https://docs.rs/noise/0.9.0/noise/struct.Perlin.html).
Les couches Perlin utilisent un melange explicite des 64 bits de seed et des
offsets stables. La version de l'algorithme fait partie du document.
Le relief historique conserve ses calculs et sa triangulation d'origine.
Sa piste suit les hauteurs canoniques apres sculpture et conserve son handle
de mesh lors des mises a jour.

Les dimensions font 8 a 256 m, la resolution 0.25 a 4 m/cellule, avec au plus
66 049 sommets (par exemple 64 x 64 m a 0.25 m). Les hauteurs restent finies
dans +/-512 m. Une scene accepte au plus 512 obstacles. Une demande excessive
est refusee, jamais convertie silencieusement en allocation non bornee.

Les objets utilisent des identifiants u64, jamais les Entity Bevy persistantes.
Leurs dimensions sont appliquees aux sommets et aux formes physiques, avec une
echelle d'entite toujours unitaire. Cubes : cuboid exact ; ellipsoides/cylindres
elliptiques : memes triangles statiques que le mesh ; rochers/rampes : enveloppe
convexe de leurs sommets. L'alignement utilise la normale et le support reel de
la forme sur le plan tangent.

## Flux Physique

1. L'interface emet une action au plus une fois par frame egui, meme si egui
   rejoue plusieurs passes. Les brosses modifient uniquement les donnees.
2. Le prochain `FixedUpdate` prepare les formes avant `PhysicsSet::SyncBackend`.
   Le vehicule est fixe et son historique est reinitialise si necessaire.
3. Rapier synchronise creations, modifications et suppressions, puis execute
   `StepSimulation`. Sa broad phase est ainsi mise a jour pour les raycasts.
4. Apres `Writeback`, la revision publiee est acquittee. Le vehicule ne peut
   reprendre ses forces qu'au pas suivant, avec un backend deja coherent.

Un `TerrainRoot` possede les entites du terrain, des objets et du decor historique.
Le vehicule, la camera et le HUD ne lui appartiennent pas. Decharger la racine
supprime les anciens colliders et les assets locaux ; les materiaux communs
sont reutilises. Le mesh de sol conserve son handle lors de la sculpture.
Les racines restent a l'identite. Les objets publient leur transformation locale
et globale ensemble, avant leur premiere synchronisation Rapier : l'ajout a une
racine existante ne depend pas d'une propagation hierarchique de fin de frame.

La reconstruction globale est bornee et limitee a 20 publications/seconde
pendant un glisser. Le dernier etat est publie au relachement ; la conduite
attend toujours cet etat. Le runtime conserve un rectangle de sommets sales
pour le futur decoupage en chunks. Les objets ne sont reconstruits qu'apres
une action qui les modifie, pas pendant chaque frame d'affichage.

## Sculpture Et Historique

Les brosses sont appliquees a pas fixe a 120 Hz ; le raycast de la souris reste
lie a l'affichage. Elever/Abaisser integrent des metres par seconde avec ce pas.
Aplanir utilise une interpolation exponentielle vers l'altitude cible ; Lisser
lit un instantane local, avec un voisinage 3 x 3, sans dependance a l'ordre
de parcours. La retombee est `(1 - distance / rayon)^exposant`. Aucun sommet
situe hors du rayon n'est touche.

Un glisser accumule un seul delta par sommet : ancienne hauteur au debut,
derniere hauteur a la fin. L'historique conserve jusqu'a 32 operations dans
96 MiB, avec annulation du retablissement quand une nouvelle branche est creee.
Les operations d'objets gardent leurs listes avant/apres, sans recopier le
terrain. Regenerer ou charger une autre scene remet cet historique a zero.

## Format JSON

Le document complet contient `format: "motor-trail-terrain"`, `format_version: 1`,
`generator_version: 1`, `name`, `terrain`, `obstacles` et `spawn`.
`terrain.config` conserve seed et parametres. `terrain.heights` contient un
**snapshot integral des hauteurs finales**, et `cells_x/cells_z` en verifient
la taille. Les edits manuels ne sont donc pas recalcules ni perdus au chargement.
`spawn.position.y` est une reference au sol, pas la hauteur du centre du chassis.
Les angles sont en degres ; l'orientation nulle pointe vers +Z.

Une version inconnue est refusee explicitement : pas de migration implicite.
Un changement futur du bruit ne modifiera pas les snapshots existants ; une
migration explicite pourra accepter leur ancienne version apres validation.
Les lectures sont limitees a 16 MiB avant deserialisation. Noms/path traversal,
liens symboliques, fichiers non reguliers, versions, valeurs non finies et
identifiants dupliques sont verifies. La sauvegarde ecrit un temporaire voisin,
le synchronise, puis le renomme. La scene active ne change qu'apres validation
complete d'un chargement.

## Tests Et Limites

```powershell
cargo fmt --all -- --check
cargo test
cargo test --release
cargo build --release
git diff --check
```

Les tests terrain couvrent les seeds, bornes, triangles, colliders, brosses,
historique, JSON, references, changements de scene dans un vrai backend Rapier,
immobilisation/reprise et stabilisation du vehicule. Les tests physiques
anterieurs restent en place. Une compilation ou un smoke test ne constitue pas
une validation manuelle du ressenti de conduite.

Le probe graphique optionnel utilise le vrai rendu, des evenements souris et
les raycasts de l'editeur, puis se ferme automatiquement :

```powershell
cargo run --release -- --terrain-smoke "$env:TEMP\motor-trail-terrain-check"
```

Il produit six PNG, un JSON de verification et des traces de sculpture,
apercu vert/rouge, rotation, annulation, placement/selection 3D, Undo/Redo et reprise de conduite. Une erreur retourne
un code de sortie non nul. Il ne modifie pas les scenes utilisateur du projet.

Validation initiale du 15 octobre 2026, avant les corrections de contacts/apercu, sous Windows 11 :

| Verification | Resultat |
| --- | --- |
| `cargo fmt --all -- --check` | Reussi |
| `cargo test` | 178 reussis, 0 echec |
| `cargo test --release` | 178 reussis, 0 echec |
| `cargo build --release` | Reussi |
| `git diff --check` et index Git | Reussis |
| Probe natif Vulkan, quatre captures inspectees | Reussi |

Le probe a verifie une modification par raycast, le placement et la selection
d'une primitive, la sauvegarde exacte, Undo/Redo et le retour en edition.
Le 4x4 a parcouru environ 10 m pendant l'intervalle de conduite automatise.
La scene finale comptait huit objets et neuf colliders de scene. Le ressenti
de conduite reste a evaluer manuellement ; ces controles n'en font pas la promesse.

### Diagnostic Et Apercu (Livraison Precedente)

Le perimetre initial retenu le 15 octobre etait le diagnostic et l'apercu uniquement.
Le controleur physique reste intact, ainsi que les formats JSON et les profils.
Les regressions precedentes de sculpture, conduite, profils, capture egui,
historique et sauvegarde sont conservees.
La base avec controleur natif passe 201 tests en mode normal et 201 en release,
sans test ignore. Les anciens resultats a 206 tests concernaient l'adaptateur
experimental retire et ne decrivent pas la livraison retenue.

Le diagnostic du GLB est decrit dans
[Diagnostic des contacts](wheel-contact-diagnosis.md). Le rayon central manque
la face de la marche avant le passage du moyeu : le defaut de penetration
reste present. Les tests de caracterisation reproduisent cette limitation ;
ils ne sont pas une preuve de correction physique. Les contacts volumetriques
verts sont exclusivement diagnostiques. Pas de rayon gonfle ni de collider de roue.

L'apercu utilise la geometrie finale, sans collider ; vert ou rouge transparent,
rotation cumulee par primitive, Maj pour le pas fin, clic droit/Echap pour
annuler. Les tests verifient aussi le nettoyage des entites et assets lors des
changements d'outil, de mode et de scene, ainsi que les entrees capturees par egui.

Le smoke graphique de l'apercu s'est termine avec six captures inspectees : apercu
vert/rouge, rotation 15 puis 10 degres, annulation avec nettoyage, confirmation
a la pose affichee, selection 3D, JSON, Undo/Redo et environ 10 m en conduite
avec l'atelier physique ouvert. Scene finale : huit objets et neuf colliders.
Windows a refuse une demande de deplacement du curseur natif durant ce probe ;
les evenements injectes, les raycasts et toutes les assertions ont neanmoins
reussi (code de sortie 0). Ce controle automatise ne remplace pas un essai du
ressenti a la main.

## Fichiers

Crees : `src/terrain/{mod,config,data,generation,mesh,brush,obstacles,scene,history,persistence,legacy,runtime}.rs`,
`src/terrain/runtime/smoke.rs`, `src/ui/{input_capture,terrain_workshop}.rs`,
`configs/terrains/Exemple Plat.json`, `configs/terrains/Exemple Relief.json`,
`docs/terrain-workshop-plan.md` et ce document.
Modifies : `Cargo.toml`, `Cargo.lock`, `src/main.rs`, `src/vehicle/mod.rs`,
`src/ui/mod.rs`, `src/ui/physics_tuning.rs` et `README.md`.
Les assets du vehicule, son controleur et les JSON de profils ne changent pas.
Le diagnostic et l'apercu ajoutent `src/terrain/runtime/placement_preview.rs`,
`src/vehicle/{contacts,diagnostics}.rs`, leurs tests/probes et
`docs/wheel-contact-diagnosis.md`. Cette etape initiale ne livrait pas d'adaptateur physique local.

### Extension Mission A

La mission suivante ajoute une carrosserie composee, les fixations et les
equipements rigides, ainsi qu'un backend pneus experimental explicitement
desactive par defaut. Le vrai controleur natif et le format JSON du terrain
restent preserves. Voir [Physique modulaire](vehicle-physics.md).
Les remises en position utilisent l'enveloppe complete installee ; une
empreinte hors carte laisse le vehicule immobilise en Edition avec une erreur.
Le rechargement du terrain conserve les equipements enfants du vehicule.

Limites de cette version : pas de streaming, de grottes ou de relief en surplomb ;
pas de gizmo de transformation complet (champs numeriques et selection 3D) ;
reconstruction globale du sol ; objets statiques ; arbres, rochers et refuge
historiques conserves aux positions d'origine, non editables comme les nouvelles
primitives (la piste, elle, suit le relief sculpte) ;
alignement sur un plan tangent, qui peut demander un ajustement sur relief tres
irregulier. Les profils physiques, SurfaceGrip et le pas fixe a 120 Hz restent
inchanges ; aucune nouvelle friction de surface complexe n'est ajoutee.

Extensions prioritaires : chunks avec bords/normales communs et dirty regions,
mesures de cout de reconstruction, gizmos de translation/rotation, surface IDs
persistants relies a SurfaceGrip puis validation de leur friction, et cache de
collisions de grandes cartes. Les snapshots peuvent devenir des fichiers
binaires par chunk tout en conservant le manifeste JSON versionne.


## Editor V2 — Catalogue et generation rapide (PR 1)

Dans **T > Edition > Objets**, le catalogue affiche les 23 GLB Nature et
les cinq primitives. Les vignettes sont projetees depuis la geometrie reelle
des objets, avec les couleurs et facettes des modeles. Ces miniatures sont
calculees et mises en cache une fois pendant la session ; il n'y a pas de
fichiers PNG additionnels a integrer dans les archives Windows/macOS.

- Rechercher le nom ou la categorie, avec une recherche insensible aux accents
  et a la casse. Filtrer par categorie avec la liste deroulante.
- Cliquer une carte pour choisir l'outil Placer et les dimensions naturelles.
- **Unique** (par defaut) : le clic dans le monde place l'objet, le selectionne,
  puis passe en outil Selectionner.
- **Multiple** : les clics successifs ajoutent des objets sans changer d'outil.
  Le dernier reste selectionne. Q/E tourne l'aperçu, Maj reduit le pas de 15 a
  5 degres ; Escape / clic droit termine la pose.
- Les clics dans l'interface ne placent pas d'objets. Les objets poses utilisent
  le meme schema JSON stable, les memes colliders et le meme historique Undo/Redo.

Dans **Generation**, Creation rapide propose quatre presets, une intensite de
relief 0-100 %, largeur et profondeur, graine u64 copiable, bouton Autre variante
et bouton Generer. La valeur 50 % correspond a l'amplitude par defaut du preset,
0 % a un terrain plat, 100 % au double de l'amplitude (borne a 256 m).
Il n'y a **aucun changement** a l'algorithme ou a la version des seeds.

Le bouton Autre variante utilise une transformation deterministe SplitMix64
du seed et ne modifie que le brouillon de generation. La scene active ne change
qu'apres **Confirmer**. Le dialogue rappelle que relief, objets et depart
courants sont remplaces ; sauvegarder la map avant est conseille. Le bouton
Annuler conserve integralement la scene active. La section Avance rassemble
l'ancien panneau avec tous ses parametres, y compris Vallee historique.

Cette livraison ne contient pas de gizmos 3D, de groupes, de prefabs, de
generation automatique de vegetation ni de nouveaux GLB. L'interface des
transformations numeriques et les outils du jeu restent disponibles.


## Editor V2 — Transformations directes (PR 2)

En mode **Edition > Objets**, selectionner un modele dans le monde ou dans la
liste. Les boutons **G · Deplacer**, **R · Tourner**, **S · Taille** choisissent
les trois gizmos ; les touches G, R, S agissent quand l'atelier est en mode
Selectionner, hors champs de saisie. Les axes visibles utilisent X rouge, Y
vert et Z bleu. Un drag reste verrouille sur la poignee choisie meme si la
souris s'en eloigne. Maj reduit l'amplitude des mouvements, Echap et clic droit
annulent le geste ; seul le relachement du clic gauche valide la modification.

Lors d'une manipulation, un apercu separé sans collider utilise la geometrie
et les couleurs d'origine. L'objet physique conserve sa pose pendant le geste.
Une seule operation Undo/Redo est creee au relachement ; annuler n'ajoute rien.
Le gizmo respecte les dimensions limitees a 0,1–32 m et ne modifie pas le
format JSON. En cas de collision visuelle ou d'intersection, les objets restent
libres de se superposer.

**Suivre le sol** garde l'alignement initial au terrain ; en desactivant cette
option et avec le champ **Decalage vertical**, on peut volontairement enfoncer
ou surélever un objet lors du placement. Le bouton historique **Aligner au sol**
reste disponible apres coup. La selection dans la liste permet de retrouver
les objets totalement enterres.

L'option **Aimantation** propose un pas de grille pour les translations et
dimensions, ainsi qu'un pas angulaire pour les rotations. **Autres objets**
aligne approximativement les boites englobantes voisines ; il ne s'agit pas
d'un assemblage exact des triangles. Maintenir Alt pendant un drag ignore
temporairement l'aimantation. **Taille uniforme** applique un facteur commun
aux trois dimensions depuis la poignee active.

### Limitations de la PR 2

Les rotations sont converties dans les trois angles Euler YXZ deja utilises
par Motor-Trail. La selection multiple, les compositions et une fusion
geometrique sont hors scope (PR 3). Les apercus du catalogue de la PR 1,
les GLB embarques et le packaging ne changent pas.

## Editor V2 — Selection multiple, groupes et compositions (PR 3)

Dans **Edition > Objets**, Maj + clic dans le monde ou la liste ajoute ou retire des
objets. Cliquer sans Maj remplace la selection. Un groupe est selectionne en
entier, meme lorsqu'on clique un de ses elements. Pour modifier un element
individuellement, utiliser **Degrouper** puis regrouper les pieces voulues.
**Tout selectionner** et **Deselectionner** sont egalement disponibles.

A partir de deux objets, donner un nom puis cliquer sur **Grouper**.
Il s'agit d'une fusion logique et non d'une union geometrique : chaque
objet conserve son mesh GLB, sa taille, son ID et son collider. Chaque objet
appartient au plus a un groupe. G, R et S manipulent l'ensemble autour
de son centre, avec les gizmos et les aimantations de la PR 2. Les
deplacements, rotations, changements de taille, duplications, groupements
et degroupements produisent un seul Undo/Redo atomique.

Les groupes sont stockes dans le champ facultatif groups de la scene JSON
format_version 1. Les anciennes maps sans ce champ restent lisibles.
La suppression nettoie les references des groupes ; ceux qui comptent moins
de deux membres sont dissous. Les references et appartenances sont validees
au chargement.

### Mes compositions

Une selection de 2 a 64 objets peut etre sauvegardee sous un nom dans le
sous-dossier utilisateur terrains/compositions/, en JSON versionne.
Les noms de fichiers sont controles, et une composition existante
ne peut pas etre ecrasee accidentellement.

Les compositions sauvegardees apparaissent sous **Mes compositions**.
**Inserer** copie leurs objets au curseur, ou pres du depart si aucun
point n'est vise, avec de nouveaux IDs et un nouveau groupe. Les meshes
sont exclusivement des assets officiels embarques, si bien que les maps
partagees restent independantes de la bibliotheque personnelle.

**Limites UX de cette PR :** la bibliotheque de compositions affiche
des lignes nommees plutot que des apercus 3D. L'insertion s'effectue
au curseur par bouton, sans ghost multi-objets avant confirmation.
Ces points peuvent etre ameliores dans la PR 4.
