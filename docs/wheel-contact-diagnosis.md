# Diagnostic Des Contacts De Pneus

## Perimetre Retenu

La premiere livraison conservait uniquement le diagnostic et l'apercu de
placement. Mission A ajoute, sur accord explicite, un mode pneus experimental
**desactive par defaut**. Le vrai controleur natif reste intact et constitue
la reference. Aucun rayon, offset du GLB ou fichier de profil n'est modifie.

## Mesures Du Modele

Les mesures proviennent du JSON et des sommets binaires du GLB livre, pas
seulement du script de generation :

| Element | Mesure |
| --- | --- |
| Rayon de gomme | 0.53000003 m |
| Rayon physique Baseline | 0.53 m |
| Rayon visuel de reference | 0.53 m |
| Rayon maximal des crampons | 0.55512388 m |
| Largeur de gomme | 0.342 m |

Le modele est translate de -1 m et retourne de PI autour de Y. L'animation
place les pivots a Y = 0.78 - longueur de suspension, soit -0.22 - longueur
par rapport au chassis. Cela correspond aux points MOUNTS du controleur.
Les correspondances FL/FR/RL/RR tiennent compte du retournement du modele.
Les tests lisent les pivots et les limites du GLB et les comparent aux centres
physiques pour plusieurs courses de suspension et une carrosserie inclinee.

La marge de crampons est d'environ 2.5 cm. Elle peut expliquer une petite
intersection de relief, mais pas un bloc remontant jusqu'au moyeu.
Les colliders des Cubes sont des cuboids exacts ; les autres primitives
partagent les dimensions bakees des meshes, avec une echelle unitaire.

## Cause Reproduite

Rapier 0.33.0-alpha lance un seul rayon descendant depuis le point d'attache de
chaque roue, sur repos + rayon. Une face verticale devant le pneu n'est pas
detectee avant le passage du moyeu au-dessus de l'arete. Le pneu raycast n'a
pas de volume de collision propre. Les dimensions et les moyeux sont coherents.

Le test de caracterisation
`wheel_step_diagnostic_reproduces_native_raycast_penetration`
utilise un vrai Cube du catalogue haut de 0.25 m. Le controleur natif produit
environ 0.253 m de penetration maximale de gomme. Un second test reproduit la
limitation en marche arriere. Ils verifient la reproduction du defaut connu,
pas un contact corrige. Les tests experimentaux distincts demandent moins
de 6.5 cm sur cette marche, dans les deux directions, avec franchissement reel.

Le probe avec le vrai GLB et Vulkan a aussi reproduit environ 0.25 m, les quatre
roues touchant le bloc. Le contact sur sol plat conserve la hauteur analytique
d'equilibre, le rayon centre/contact et la charge totale attendue. Augmenter le
rayon ou decaler le vehicule masquerait donc la cause sans la traiter.

## Diagnostic Visuel

`V` active ou desactive l'affichage, hors saisie dans egui. Aucun reglage n'est
enregistre dans les profils. On peut aussi l'activer au demarrage :

```powershell
cargo run --release -- --wheel-diagnostics
```

- Blanc : centre physique de chaque roue avec sa suspension courante.
- Cyan : rayon de gomme et largeur physique, avec le braquage courant.
- Rouge : origine et direction du rayon descendant natif.
- Orange : contact actuellement utilise par Rapier et liaison au centre.
- Vert clair : contact et centre candidats du balayage volumetrique de diagnostic.
- Filaire du bloc : collider Rapier reel.

Les marqueurs restent visibles a travers les pneus. Le contact du controleur
est le dernier releve physique ; le chassis affiche a deja termine ce pas a
120 Hz. Toutes les requetes de diagnostic sont en lecture seule, sans impulsion,
collider de roue ou changement de suspension. L'option experimentale est
independante de cet affichage ; seule elle change le contact choisi.

Le probe charge une scene et un bloc temporaires en memoire, sans sauvegarder
de profil ou de scene utilisateur. Par defaut il caracterise le defaut natif :

```powershell
cargo run --release -- --wheel-contact-probe "$env:TEMP\motor-trail-wheel-check"
```

## Integration Experimentale

`src/vehicle/contacts.rs` contient une requete geometrique sans forces utilisee
par le diagnostic, les tests et le backend experimental. Elle balaie un cylindre au rayon et a la largeur
reels du pneu, suivant la suspension et le braquage. Elle detecte l'arete avant
le rayon central. La reprojection du plan de contact compense la tolerance
conservative de GJK sans gonfler le pneu. Les tests geometriques couvrent marche,
sol plat, rocher du catalogue, braquage, largeur, translation/rotation, entrees
invalides et recherche d'un appui sous un autre collider en recouvrement.

L'API Rapier expose les contacts des roues en lecture seule. Le mode experimental
utilise donc `experimental_tires.rs`, derive isole du controleur epingle, avec
provenance et licence conservees. `wheel_backend.rs` delegue au vrai controleur
amont en mode Native ; il ne remplace pas silencieusement ce dernier par une copie.
L'option est accessible dans l'onglet Pneus ou par `--experimental-tires`.
Elle ne persiste pas dans les profils. Les calculs existants restent uniques :

- Selectionner un seul contact avant vitesse relative, amortissement et charge.
- Garder le rayon central quand les contacts sont equivalents sur sol plat.
- Comparer les courses avant leur limitation, meme si le minimum est negatif.
- Rejeter les contacts degeneres ou presque lateraux sans masquer d'autres appuis.
- Conserver plafonds, cercle de friction, freinage et maintien en pente.
- Tester la parite des forces avec le vrai controleur, et le franchissement avant/arriere.

Les caches de roues sont prealloues. Le rejet des contacts utilise un tampon
fixe de 32 colliders ; au-dela, le balayage abandonne et le rayon natif reste
disponible. Les geometries de penetration initiale ne sont pas calculees, car
ces contacts sont toujours rejetes. Un test instrumente l'allocateur du thread
et verifie zero allocation sur 32 requetes repetees avec plafond a exclure.
Si plafond et appui appartiennent au meme compose, ses sous-formes simples
sont interrogees sans allocations, dans un budget global de 64 sous-formes.
Les composes imbriques ne sont pas parcourus. Un TriMesh/heightfield conserve
la limite de sa premiere feature touchee ; le rayon natif reste le repli.

Sur la marche de 25 cm du catalogue, les tests mesurent environ 0.037 m en
avant et 0.033 m en arriere, contre 0.253 et 0.250 m en natif. Le test de
marche de 40 cm controle surtout la stabilite et les valeurs finies, pas une
capacite garantie de franchissement (environ 9 cm de penetration dans cet essai).

```powershell
cargo run --release -- --experimental-tires --wheel-contact-probe "$env:TEMP\motor-trail-envelope"
```

Le probe experimental verifie son backend actif et une penetration inferieure
a 7 cm ; le probe natif conserve son role de caracterisation du defaut.

Un balayage rigide ne simule ni la gomme ni des roues en collision continue.
Crampons, butees, chocs rapides, parois et surplombs restent des limites.
En basculement sur un flanc, les roues suspendues n'ont pas de contact lateral
rigide complet. Le probe natif mesure environ 11 cm d'intersection d'un moyeu
au premier impact ; la carrosserie et les equipements sont controles separement.
Ajouter des colliders physiques aux roues sans revoir les forces risquerait
de dupliquer l'appui ou la friction.

Le GLB, l'animation des roues, les dimensions et les profils JSON restent
inchanges. La carrosserie physique complete et les equipements font l'objet
de [leur documentation propre](vehicle-physics.md).
