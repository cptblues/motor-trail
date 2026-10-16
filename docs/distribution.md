# Distribution Native

## Pipeline

`.github/workflows/release.yml` ne declare qu'un evenement : push de tag `v*`.
Il n'a ni branche, ni PR, ni declenchement manuel. `rust.yml` conserve sa CI
habituelle ; `generate-vehicle.yml` n'est pas modifie.

| Runner natif | Cible | Archive |
| --- | --- | --- |
| windows-latest | x86_64-pc-windows-msvc | Motor-Trail-Windows-x64.zip |
| macos-15 (ARM) | aarch64-apple-darwin | Motor-Trail-macOS-AppleSilicon.zip |
| macos-15-intel | x86_64-apple-darwin | Motor-Trail-macOS-Intel.zip |

Labels et architectures verifies le 15 octobre 2026 dans les
[runners GitHub](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)
et les [images officielles](https://github.com/actions/runner-images).
Chaque job verifie le host de `rustc -vV`, installe Rust stable et utilise
`cargo build --release --locked --target <cible>`, avec un cache Cargo par cible.
Windows utilise `-C target-feature=+crt-static` pour ne pas exiger l'installation
du runtime C MSVC. Les dependances systeme graphiques restent necessaires.
Le commit exporte provient de `git rev-parse HEAD` et doit correspondre au tag
deplie avec `^{commit}`, y compris pour un tag annote.

Les builds ont seulement `contents: read`. Apres reussite des trois builds,
le job de publication telecharge et verifie exactement les trois archives,
puis cree un brouillon, charge les trois fichiers et publie. Lui seul recoit
`contents: write`, avec `GITHUB_TOKEN` ; aucun token personnel n'est utilise.
Un echec obligatoire empeche la publication. Un echec pendant le chargement
peut laisser un brouillon, mais jamais une nouvelle Release publique incomplete.
Une relance apres ce cas demande de verifier le brouillon existant ; le workflow
ne remplace pas aveuglement une Release deja presente.

`--verify-tag` interdit de creer implicitement un tag lors de la publication.
Les tags `vMAJOR.MINOR.PATCH` acceptent des suffixes, par exemple
`v0.1.0-alpha.1`. Un tag `v*` sans version exploitable declenche le workflow mais
echoue aux controles. `alpha`, `beta` ou `rc`, sans distinction de casse,
marquent la Release comme prerelease.

## Contenu Des Archives

Windows contient `Motor-Trail/motor-trail.exe`, `assets/`, `defaults/vehicles/`,
`defaults/terrains/`, `README.txt` et `version.json` sous le meme dossier.

macOS contient une vraie application `Motor-Trail.app` avec :

```text
Contents/
  Info.plist
  MacOS/motor-trail
  Resources/
    assets/vehicles/motor_trail_4x4.glb
    defaults/vehicles/
    defaults/terrains/
    README.txt
    version.json
```

Le bundle declare `com.motortrail.game`, affiche `Motor-Trail`, conserve une
version numerique dans les cles standard et le tag complet dans une cle dediee.
`version.json` contient le tag, la cible et le commit source. Les permissions
executables macOS sont conservees dans le ZIP. Faute d'icone existante, le bundle
utilise l'icone generique du systeme, sans reference vers un fichier absent.

La liste explicite des ressources inclut le vrai GLB, les cinq profils physiques,
les fixations, les deux exemples Terrain, les 23 GLB Nature et leur catalogue.
Voir [les assets Nature](nature-assets.md) pour cette integration.
Elle exclut generateurs, fichiers
temporaires, selection du dernier profil et JSON personnels non references.
L'ajout futur de ressources necessite de mettre a jour cette liste et ses tests.
Le validateur controle notamment architectures PE/Mach-O, GLB, JSON, plist,
version, fichiers attendus, CRC, chemins ZIP et absence de liens/entrees doubles.

## Chemins Et Donnees

Les chemins ne dependent pas du repertoire courant :

- ZIP Windows : ressources voisines de l'executable.
- Bundle macOS : `Contents/Resources`.
- Developpement : racine absolue du manifeste Cargo, meme avec `target` deplace.
- `BEVY_ASSET_ROOT` reste utilisable en developpement avec un chemin absolu.
  Une distribution incomplete ne se replie pas sur un depot de developpement.

Les ressources livrees sont des references lues seulement par le jeu. Les
copies modifiables sont stockees sous :

| Systeme | Racine utilisateur |
| --- | --- |
| Windows | %APPDATA%/MotorTrail/ |
| macOS | ~/Library/Application Support/MotorTrail/ |
| Linux, developpement/CI | $XDG_DATA_HOME/MotorTrail/ ou ~/.local/share/MotorTrail/ |

`vehicles/` contient profils et dernier profil choisi ; `terrains/` contient
les scenes sauvegardees. `settings/` et `community_maps/` sont reserves aux
extensions futures, sans ajout de nouvelle fonctionnalite dans cette mission.
Les scenes de reference generees restent protegees en ecriture.

Au premier demarrage, les fichiers reconnus de l'ancien `configs/` associe au
depot ou a l'installation sont copies avant les references manquantes. Les
fichiers deja presents dans la racine utilisateur ont toujours priorite.
Les sources restent intactes ; les fixations ne sont pas prises pour un profil.
Les copies sont publiees atomiquement sans remplacement, y compris face a une
sauvegarde concurrente. Une erreur laisse les sources disponibles et ne marque
pas la migration terminee. Un marqueur de succes evite de recopier ensuite une
scene volontairement supprimee. Les liens/reparse points ne sont pas suivis
pour migrer ou sauvegarder dans ces emplacements.

Pour des donnees anciennes restees dans un autre depot/dossier, les originaux
ne sont pas recherches sur tout le disque : demarrer une fois depuis cet ancien
depot mis a jour, ou importer manuellement ses JSON dans la racine utilisateur.
Ne pas supprimer les originaux avant d'avoir verifie la copie.

Un test manuel peut isoler ses donnees avec :

```powershell
cargo run --release -- --user-data-dir "C:/Temp/MotorTrail-Test"
```

Sans racine utilisateur valide/accessible ou avec une installation corrompue,
le demarrage signale une erreur au lieu d'ecrire dans le dossier d'installation.

## Controles Sans Interface

```powershell
cargo run --release -- --check-installation
python -B -m unittest discover -s scripts/tests -v
```

Sous Windows, utiliser `py -3` a la place de `python` si le lanceur Python est
installe. Le script requiert Python 3.10+ pour construire les archives ; Python
n'est pas requis sur la machine du joueur.

Le controle `--check-installation` valide effectivement les cinq profils,
fixations, deux scenes et le GLB par leurs parseurs Rust, sans fenetre, GPU ni
creation de donnees utilisateur. Chaque runner extrait sa propre archive,
lance ce controle depuis un autre dossier temporaire et verifie l'absence
d'ecritures. La validation structurelle ne prouve pas que le rendu est correct.
En developpement, les anciens JSON modifiables ne bloquent pas ce controle :
les profils/fixations integres sont verifies et les anciens fichiers sont
laisses a la migration puis au repli existant de ProfileStore. Un profil invalide
reste intact et signale ; un profil manquant peut etre reconstitue depuis les
valeurs integrees. Les applications distribuees verifient strictement tous
leurs fichiers de reference. Le GLB est controle jusque dans ses plages de
donnees binaires, sans faire confiance aux seules descriptions JSON.
Le controle est borne au modele livre : acces denses scalaires/vectoriels,
geometrie integree et budget conservateur de 256 MiB pour les valeurs des
accesseurs. Le calcul reserve au moins 16 octets par element pour couvrir les
conversions vers Float32x4 ; ce n'est pas un plafond de memoire du moteur entier.
Les acces matriciels ou
parcimonieux sont refuses explicitement ; leur support futur demande des tests
dedies, sans reimplementer ici un importeur glTF generaliste.

Exemple local de packaging Windows, apres une compilation native de distribution :

```powershell
$env:MOTOR_TRAIL_DISTRIBUTION = '1'
$env:RUSTFLAGS = '-C target-feature=+crt-static'
cargo build --release --locked --target x86_64-pc-windows-msvc
$sourceCommit = (git rev-parse HEAD).Trim()
py -3 scripts/package_release.py build --binary target/x86_64-pc-windows-msvc/release/motor-trail.exe --target x86_64-pc-windows-msvc --tag v0.1.0-alpha.1 --commit $sourceCommit --output-dir dist
py -3 scripts/package_release.py smoke dist/Motor-Trail-Windows-x64.zip --target x86_64-pc-windows-msvc --tag v0.1.0-alpha.1 --commit $sourceCommit
Remove-Item Env:MOTOR_TRAIL_DISTRIBUTION,Env:RUSTFLAGS
```

Ces commandes locales ne creent aucun tag ni Release. Enlever les variables
apres le test permet de reprendre le lancement de developpement habituel.

## Verification Manuelle

Sur une vraie machine Windows sans Rust/Cargo : extraire le ZIP complet,
double-cliquer, verifier modele/textures, conduire, essayer l'atelier physique,
sculpter et placer un obstacle, sauvegarder un profil et une scene. Relancer
depuis un autre dossier puis remplacer l'installation : les sauvegardes et la
selection du profil doivent rester disponibles.

Sur chaque architecture macOS : telecharger/extraire avec le Finder, verifier
que l'application est reconnue, deplacer le bundle dans Applications, lancer et
refaire les controles ci-dessus, puis verifier les sauvegardes sous Application
Support et apres mise a jour. Le test CLI n'est pas un test de double-clic Finder.

Les applications sont non signees et non notarisees ; Gatekeeper peut bloquer
l'ouverture. Consulter [Ouvrir des apps en toute securite](https://support.apple.com/fr-fr/102445)
pour comprendre les avertissements et verifier la provenance, sans commande
de suppression de quarantaine ni desactivation de Gatekeeper. Aucun certificat
ni secret de signature n'est requis par le workflow.

## Verification Locale Du 16 Octobre 2026

Les controles suivants ont ete executes sur Windows, sur les changements locaux :

| Controle | Resultat |
| --- | --- |
| `cargo test --quiet` | 316 tests reussis |
| `cargo test --release --quiet` | 316 tests reussis |
| Tests Python de `scripts/tests` avec PyYAML disponible | 30 tests reussis, aucun ignore ; structure YAML verifiee |
| `cargo fmt --all -- --check` | Reussi |
| Build natif Windows release, `--locked`, cible MSVC et CRT statique | Reussi |
| Creation/validation du vrai ZIP Windows | Reussie, 32 395 744 octets |
| Controle du ZIP extrait depuis un autre dossier, sans Rust/Cargo dans PATH et avec BEVY_ASSET_ROOT invalide | Reussi, aucune ecriture du controle sans interface |
| `cargo run --release -- --check-installation` et executable de developpement depuis un autre dossier | Reussis |
| Imports DLL de l'executable de distribution | Bibliotheques Windows uniquement, pas de runtime C MSVC externe |
| Test graphique du ZIP extrait, donnees isolees, sans Rust/Cargo dans PATH | Code de sortie 0, six captures |

Le test graphique a verifie sculpture, aller-retour JSON, apercus valide/invalide,
rotation 15 vers 10 degres, confirmation, selection, huit objets/neuf colliders,
atelier physique et conduite d'environ 10 metres. Les captures ont ete inspectees
pour le chargement de la scene et du vehicule. Les tests de regression existants
de physique, d'equipements et de sauvegarde ont egalement ete conserves.
Quatre avertissements `dead_code` preexistants restent dans le code vehicule ;
aucun n'a ete masque ni corrige hors du perimetre de cette mission.

Le ZIP local utilise la chaine de test `v0.1.0-local-check` : ce n'est pas un tag
Git et cette archive issue du travail local n'est pas une Release publiee.
Les deux builds macOS natifs, le lancement Finder/Gatekeeper et l'execution du
workflow GitHub restent a verifier sur leurs environnements reels. Les tests de
structures macOS utilisent des fixtures, pas des compilations macOS locales.
Aucun tag, push, Release, secret ou parametre du depot n'a ete cree ou modifie.

## Fichiers De La Mission

| Fichiers | Role |
| --- | --- |
| `.github/workflows/release.yml` | Matrice native, controles et publication reservee aux tags |
| `scripts/package_release.py` | Construction, validation et controle des archives extraites |
| `scripts/tests/test_package_release.py`, `scripts/tests/test_release_workflow.py` | Tests du packaging et des regles de publication |
| `src/app_paths.rs`, `src/app_paths/migration.rs`, `src/app_paths/validation.rs`, `src/app_paths/tests.rs` | Resolution des ressources, donnees utilisateur, migration et tests |
| `src/main.rs` | Integration des chemins et du controle sans interface au demarrage |
| `src/terrain/persistence.rs`, `src/terrain/runtime.rs` | Injection du dossier de scenes utilisateur et test sauvegarde/chargement |
| `src/persistence/profiles.rs`, `src/terrain/scene.rs` | Reutilisation des validateurs de noms existants pour la migration |
| `README.md`, `docs/distribution.md`, `.gitignore` | Instructions et exclusion des sorties locales |
| `docs/superpowers/specs/2026-10-15-native-distribution-design.md`, `docs/superpowers/plans/2026-10-15-native-distribution.md` | Conception approuvee et suivi de verification |

`Cargo.toml`, `Cargo.lock`, `src/vehicle/`, les ressources sources et les deux
workflows existants restent inchanges.

## Premiere Release

Une fois ces changements integres et la CI verifiee sur `main`, utiliser un tag
inexistant sur un commit teste :

```bash
git checkout main
git pull
git tag v0.1.0-alpha.1
git push origin v0.1.0-alpha.1
```

Les executions macOS et la publication reelle ne peuvent etre declarees validees
qu'apres un futur push de tag. Aucun tag ni Release n'est cree pendant la mise
en place du pipeline.
