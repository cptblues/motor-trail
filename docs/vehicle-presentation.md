# Synchronisation Visuelle Du Vehicule

## Cause Identifiee

Le corps physique, le GLB et les roues affichaient directement le dernier pas
Rapier a 120 Hz, tandis que la camera etait lissee a chaque image. A 144 ou
240 images/s, certaines images repetaient exactement la pose precedente, puis
sautaient vers la suivante. Un test a 144 images/s a reproduit cette repetition
sur une roue en mouvement avant la correction, puis passe apres celle-ci.
Ce defaut existe independamment du choix pneus natifs / experimentaux.

## Correction

`vehicle/presentation.rs` capture les poses avant/apres chaque pas fixe et
interpole avec `Time<Fixed>::overstep_fraction()`. Le GLB, les suspensions, la
direction, la rotation des quatre roues et les graphismes des equipements
utilisent le meme instant. La rotation des roues interpole le chemin court au
passage de zero, dans les deux sens. La camera suit cette meme pose affichee.

Seuls des enfants graphiques recoivent une compensation locale. Le corps
rigide et les parents physiques des equipements gardent leur vraie pose.
Le pas reste a 120 Hz, avec les memes rayons, appuis, suspensions, adherence,
motricite et application unique des forces. Aucun lissage des forces ou du
choix de contact n'a ete ajoute.

L'interpolation introduit un retard visuel d'un pas fixe, soit environ 8.3 ms,
sans retard supplementaire accumule. Les gizmos de diagnostic et la telemetrie
representent toujours le dernier etat physique, pas l'etat interpole.
Une remise a zero, un nouveau vehicule ou une modification de geometrie/mode
invalide les anciens echantillons : pas de trajet visuel depuis l'ancien spawn.

## Roue Coincee

Deux blocs symetriques de 25 cm, separes par un espace de 80 cm, permettent
de tester deux appuis admissibles alors que le rayon central manque les blocs.
Les requetes repetees sans mouvement retournent exactement le meme contact.
En deplacant le moyeu de -0.1 a +0.1 mm, la selection peut changer de bloc :
ecart de longueur d'environ 0.023 mm, mais changement de normale d'environ
1.50 en norme vectorielle (normales longitudinales opposees).

Le montage dynamique contraint a une seule roue, avec un vrai sol sous les
blocs, enregistre 55 changements de collider en 240 pas (2 s), un saut maximal
de longueur de 1.67 cm et de normale de 1.53. Il reste fini mais n'est pas un
equilibre stable : le petit chassis de test descend jusqu'au sol avant de
rebondir. Sans sol, ce montage sous-supporte perd l'appui et tombe ; une roue
raycast n'est pas un collider rigide fermant l'espace entre les obstacles.
Le test garde donc le sol comme le jeu et verifie une integration effective,
pas seulement des vitesses qui changent dans un monde qui ne bouge pas.

Cela etablit une sensibilite de selection, pas la reproduction certaine du
tremblement du vehicule complet observe par l'utilisateur. L'interpolation
supprime les sauts d'affichage entre pas ; elle ne garantit pas la disparition
d'une oscillation physique entre deux appuis. Les pneus experimentaux restent
un support unique par roue, et non une roue rigide resolvant plusieurs contacts.
Une eventuelle stabilisation de selection doit etre une correction physique
distincte, verifiee sur un cas reproductible, sans conserver d'appui obsolete
ni ajouter une seconde application de forces.

## Verification

Les tests de presentation couvrent 30/60/144/240 images/s, la continuite entre
pas, la trajectoire physique strictement identique avec/sans presentation,
la camera, les deux backends de pneus, les rotations autour de zero, le reset,
le changement de rayon, la suppression/recreation du vehicule et la separation
graphismes/colliders des equipements. Des tests de caracterisation couvrent
aussi les contacts entre deux obstacles et les coutures d'un vrai TriMesh plat.
Sur les deux diagonales du TriMesh plat teste, l'erreur maximale de longueur
du balayage est de 0.043 mm et celle de normale de 0.001013 en norme vectorielle.
Le backend conserve exactement le contact et les forces du rayon natif sur
ces coutures, y compris pour une course brute negative.

Ces controles ne mesurent pas le debit GPU et ne remplacent pas un nouvel essai
manuel du ressenti sur le parcours de l'utilisateur.

## Essais Graphiques

Les probes du vrai GLB ont termine avec sortie 0 :

- Pneus experimentaux : 13.258 m en 4.8 s simulees, quatre roues sur le bloc,
  penetration maximale 1.863 cm, identique a la mesure avant interpolation.
- Carrosserie/coffre : quatre captures, masse 1840 kg, suivi physique exact
  de l'equipement et contacts de toit/flanc conserves.
- Atelier Terrain : six captures, sculpture par raycast, apercu et selection,
  rotation 15 puis 10 degres, sauvegarde JSON, historique et 10.0 m de conduite.

Les captures de contact et de carrosserie/coffre ont ete inspectees. Les sorties
restent dans les dossiers `presentation-envelope`, `presentation-body` et
`presentation-terrain` des visualisations de cette tache. Les probes utilisent
une horloge simulee : leur compteur FPS ne mesure pas les performances reelles.

Les suites completes `cargo test` et `cargo test --release` passent toutes deux :
299 tests, aucun echec et aucun test ignore. Le formatage est verifie par
`cargo fmt --all -- --check`. Aucun parametre physique, profil JSON, GLB ou
dependance n'a ete modifie dans cette correction de presentation.
`cargo build --release` et `git diff --check` reussissent egalement ; les quatre
avertissements `dead_code` deja presents dans la Mission A restent inchanges.
