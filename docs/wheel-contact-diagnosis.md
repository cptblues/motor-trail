# Diagnostic Des Contacts De Pneus

## Perimetre Retenu

Sur demande de l'utilisateur, livrer d'abord le diagnostic et l'apercu de
placement, en gardant le controleur intact. L'adaptateur physique experimental
a ete retire, ainsi que ses tests de parite et sa copie des sources Rapier.
Le defaut de penetration des pneus n'est donc pas corrige dans cette livraison.
Aucun profil, rayon, offset de carrosserie ou calcul de forces n'est modifie.

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
pas un contact corrige. L'ancien critere d'acceptation de moins de 6.5 cm
reste un objectif pour un futur correctif physique, pas un test annonce reussi.

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
- Vert clair : contact et centre candidats du prototype volumetrique, non utilises par la physique.
- Filaire du bloc : collider Rapier reel.

Les marqueurs restent visibles a travers les pneus. Le contact du controleur
est le dernier releve physique ; le chassis affiche a deja termine ce pas a
120 Hz. Toutes les requetes de diagnostic sont en lecture seule, sans impulsion,
collider de roue, remplacement de contact ou changement de suspension.

Le probe charge une scene et un bloc temporaires en memoire, sans sauvegarder
de profil ou de scene utilisateur. Il mesure le defaut natif, pas sa correction :

```powershell
cargo run --release -- --wheel-contact-probe "$env:TEMP\motor-trail-wheel-check"
```

## Piste De Correction Differee

`src/vehicle/contacts.rs` contient uniquement une requete geometrique utilisee
par le diagnostic et les tests. Elle balaie un cylindre au rayon et a la largeur
reels du pneu, suivant la suspension et le braquage. Elle detecte l'arete avant
le rayon central. La reprojection du plan de contact compense la tolerance
conservative de GJK sans gonfler le pneu. Les tests geometriques couvrent marche,
sol plat, rocher du catalogue, braquage, largeur, translation/rotation, entrees
invalides et recherche d'un appui sous un autre collider en recouvrement.

L'API Rapier expose les contacts des roues en lecture seule. Une integration
future devrait donc offrir un point de remplacement de contact avant le calcul
des forces, par une API adaptee ou un derive local explicitement accepte.
Elle devrait conserver un seul calcul des forces et les equations existantes :

- Selectionner un seul contact avant vitesse relative, amortissement et charge.
- Garder le rayon central quand les contacts sont equivalents sur sol plat.
- Comparer les courses avant leur limitation, meme si le minimum est negatif.
- Rejeter les contacts degeneres ou presque lateraux sans masquer d'autres appuis.
- Conserver plafonds, cercle de friction, freinage et maintien en pente.
- Prouver la parite des forces et le franchissement avant/arriere par des tests.

Un balayage rigide ne simule ni la gomme ni des roues en collision continue.
Crampons, butees, chocs rapides, parois et surplombs restent des limites.
Ajouter des colliders physiques aux roues sans revoir les forces risquerait
de dupliquer l'appui ou la friction.

Le GLB, `controller.rs`, `visuals.rs`, les dimensions et les profils JSON restent
intacts dans cette livraison.
