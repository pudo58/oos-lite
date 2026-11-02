# OOS-Lite

[English](../../README.md) | [简体中文](README.zh-CN.md) | [日本語](README.ja.md) | **Français** | [한국어](README.ko.md) | [Tiếng Việt](README.vi.md)

[![CI](https://github.com/pudo58/oos-lite/actions/workflows/ci.yml/badge.svg)](https://github.com/pudo58/oos-lite/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/pudo58/oos-lite)](https://github.com/pudo58/oos-lite/releases)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../LICENSE)
[![Rust](https://img.shields.io/badge/rust-2021-orange.svg)](https://www.rust-lang.org/)
[![Support OOS-Lite](https://img.shields.io/badge/support-SOL%20%7C%20USDT-087f5b.svg)](https://pudo58.github.io/oos-lite/support/)

> Ce document est une traduction du [README anglais](../../README.md). En cas de divergence, la version anglaise fait foi.

OOS-Lite est un coffre-fort de fichiers local, adressé par contenu, écrit en Rust. Il conserve toutes les versions de vos fichiers, les déduplique par blocs (chunks) et crée des instantanés sans copier les données. Vous pouvez l'utiliser en ligne de commande, via une application de bureau, un tableau de bord web, ou le monter comme lecteur virtuel en lecture seule.

---

## Sommaire

- [Fonctionnalités](#fonctionnalités)
- [Plateformes prises en charge](#plateformes-prises-en-charge)
- [Installation](#installation)
- [Démarrage rapide](#démarrage-rapide)
- [Référence des commandes](#référence-des-commandes)
- [Architecture](#architecture)
- [Chiffrement](#chiffrement)
- [Interface web et partage public](#interface-web-et-partage-public)
- [Organisation sur disque](#organisation-sur-disque)
- [Structure du projet](#structure-du-projet)
- [Développement](#développement)
- [Contribuer](#contribuer)
- [Licence](#licence)

---

## Fonctionnalités

- **Déduplication par bloc.** Les fichiers sont découpés avec FastCDC (16 Kio minimum, 64 Kio en moyenne, 256 Kio maximum) et chaque bloc est identifié par son hachage BLAKE3. Si vous modifiez une partie d'un gros fichier, seuls les blocs modifiés sont stockés.
- **Historique des versions.** Chaque `put` sur un nom existant ajoute une nouvelle version. Vous pouvez lire, restaurer (rollback) ou élaguer les anciennes versions.
- **Instantanés.** Un instantané enregistre la version de chaque fichier à un instant donné. Il ne copie pas les données des fichiers.
- **Reprise après plantage.** Les écritures passent par un journal d'écriture anticipée (WAL) de type redo, synchronisé avec fsync avant la mise à jour des métadonnées. Au redémarrage, les opérations validées sont rejouées et une fin d'écriture incomplète dans le segment le plus récent est tronquée. La reprise est couverte par des tests d'arrêt brutal puis redémarrage.
- **Compression.** Les blocs sont compressés avec Zstandard lorsque cela fait gagner au moins 5 %. Le hachage est toujours calculé sur les données non compressées, donc la compression n'affecte pas la déduplication.
- **Chiffrement au repos (optionnel).** Le contenu des blocs et les enregistrements du WAL sont chiffrés avec XChaCha20-Poly1305, sous une clé maître protégée par Argon2id. Voir [Chiffrement](#chiffrement) pour savoir précisément ce qui est couvert.
- **Ramasse-miettes.** Le GC de type mark-and-sweep copie les blocs encore utilisés dans de nouveaux segments et les met en place avec un protocole de renommage résistant aux plantages.
- **Vérification d'intégrité.** `fsck` vérifie les enregistrements de segment, les sommes de contrôle, les manifestes et, pour les dépôts chiffrés, les étiquettes d'authentification.
- **Surveillance Auto-Vault.** Surveille un dossier et enregistre automatiquement de nouvelles versions, avec anti-rebond, délai minimal entre versions, limitation des E/S et prise en charge de `.oosignore`.
- **Lecteur virtuel.** Montez le dépôt en lecture seule via WebDAV sous Windows (sans pilote noyau) ou via FUSE sous Linux/macOS.
- **Application de bureau et interface web.** Un tableau de bord pour parcourir les fichiers, versions et instantanés, téléverser des fichiers et lancer la maintenance. Interface en anglais et en vietnamien.

## Plateformes prises en charge

| Plateforme | Version précompilée | Montage | Testé en CI |
| :--- | :--- | :--- | :--- |
| Windows x86_64 | Installateur et `.zip` portable | WebDAV | Oui |
| Linux x86_64 (glibc) | `.tar.gz` | FUSE 3 | Oui |
| macOS | Compilation depuis les sources | FUSE (macFUSE) | Non |

Les binaires Windows ne sont pas encore signés ; SmartScreen peut donc afficher un avertissement « Éditeur inconnu ».

## Installation

### Windows

Téléchargez `OOS-Lite-Setup-<version>.exe` depuis la [page des versions](https://github.com/pudo58/oos-lite/releases) et lancez-le. L'installateur contient l'outil en ligne de commande (`oos-lite.exe`) et l'application de bureau (`oos-lite-gui.exe`). Il peut aussi ajouter OOS-Lite au `PATH` et au menu contextuel de l'Explorateur.

Pour une installation portable, extrayez `oos-lite-windows-x86_64-<version>.zip` puis exécutez `.\oos-lite.exe --version`.

### Linux

Installez les bibliothèques d'exécution (Ubuntu/Debian) :

```bash
sudo apt update
sudo apt install -y libfuse3-3 libwayland-client0 libxkbcommon0 libdbus-1-3
```

Téléchargez, vérifiez et installez pour l'utilisateur courant (remplacez `v1.0.0` par la version souhaitée) :

```bash
VERSION=v1.0.0
wget https://github.com/pudo58/oos-lite/releases/download/$VERSION/oos-lite-linux-x86_64-$VERSION.tar.gz
wget https://github.com/pudo58/oos-lite/releases/download/$VERSION/SHA256SUMS.txt
sha256sum --check SHA256SUMS.txt --ignore-missing

mkdir -p "$HOME/.local/share/oos-lite" "$HOME/.local/bin"
tar -xzf oos-lite-linux-x86_64-$VERSION.tar.gz -C "$HOME/.local/share/oos-lite"
ln -sf "$HOME/.local/share/oos-lite/oos-lite" "$HOME/.local/bin/oos-lite"
ln -sf "$HOME/.local/share/oos-lite/oos-lite-gui" "$HOME/.local/bin/oos-lite-gui"
```

Vérifiez que `$HOME/.local/bin` est dans votre `PATH`.

### Depuis les sources

Il faut une chaîne d'outils Rust stable récente et un éditeur de liens C (MSVC ou MinGW sous Windows). Sous Linux, la prise en charge de FUSE nécessite aussi `libfuse3-dev` et `pkg-config`.

```bash
git clone https://github.com/pudo58/oos-lite.git
cd oos-lite
cargo build --release -p oos-lite
```

Les binaires sont générés dans `target/release/oos-lite` et `target/release/oos-lite-gui` (avec l'extension `.exe` sous Windows).

## Démarrage rapide

```bash
# Créer un dépôt (./.oos-store par défaut ; modifiable avec --store-dir)
oos-lite init

# Enregistrer un fichier, le modifier, puis l'enregistrer à nouveau
oos-lite put report.docx
oos-lite put report.docx

# Afficher ses versions et récupérer la première
oos-lite versions report.docx
oos-lite get report.docx ./report_v1.docx --version 1

# Créer un instantané de tout le dépôt, puis voir l'espace économisé par la déduplication
oos-lite snapshot create before-cleanup
oos-lite stats
```

## Référence des commandes

Options globales : `--store-dir <path>` (alias `--store`, `.oos-store` par défaut), `--password <pass>`, `--password-file <file>`. La phrase secrète peut aussi être fournie via la variable d'environnement `OOS_PASSWORD`.

### Fichiers et versions

| Commande | Description |
| :--- | :--- |
| `oos-lite init` | Initialise un dépôt (chiffré si une phrase secrète est fournie) |
| `oos-lite put <path> [--name <name>]` | Enregistre un fichier ou en ajoute une nouvelle version |
| `oos-lite get <name\|id> <out> [--version N]` | Écrit un fichier (dernière version ou version donnée) sur le disque |
| `oos-lite list` | Liste les fichiers enregistrés |
| `oos-lite versions <name\|id>` | Affiche l'historique des versions d'un fichier |
| `oos-lite rollback <name> --version N [--out <path>]` | Refait d'une ancienne version la plus récente, avec écriture optionnelle sur le disque |
| `oos-lite rm <name>` | Retire un nom du dépôt (l'historique reste jusqu'à `prune` et `gc`) |
| `oos-lite prune [--keep N] [name]` | Ne conserve que les N dernières versions (10 par défaut) |

### Instantanés

| Commande | Description |
| :--- | :--- |
| `oos-lite snapshot create <label>` | Crée un instantané |
| `oos-lite snapshot list` | Liste les instantanés |
| `oos-lite snapshot restore <label> <dir>` | Restaure un instantané dans un dossier |
| `oos-lite snapshot delete <label>` | Supprime un instantané |

### Maintenance

| Commande | Description |
| :--- | :--- |
| `oos-lite stats` | Taille logique, taille physique, taux d'économie, nombre de blocs |
| `oos-lite gc` | Récupère l'espace des blocs qui ne sont plus référencés |
| `oos-lite fsck` | Vérifie l'intégrité des données |

### Applications, montage et intégration

| Commande | Description |
| :--- | :--- |
| `oos-lite app` | Lance l'application de bureau (alias `desktop`, `gui`) |
| `oos-lite ui [--host 127.0.0.1] [--port 3000] [--no-open]` | Démarre l'interface web |
| `oos-lite mount [Z: \| /mnt/dir] [--webdav] [--port 8080] [--cache-mb 128]` | Monte le dépôt en lecture seule |
| `oos-lite watch <dir> [--debounce-secs 3] [--cooldown-secs 60] [--throttle-ms 10]` | Versionne automatiquement un dossier |
| `oos-lite shell-ext enable\|disable\|status` | Menu contextuel de l'Explorateur Windows |
| `oos-lite stop` | Arrête les processus OOS-Lite en arrière-plan et libère les ports 3000/8080 |
| `oos-lite uninstall` | Arrête les processus et supprime les composants installés |

Lancez `oos-lite <command> --help` pour la liste complète des options.

Le lecteur monté contient trois dossiers : `current/` (dernière version de chaque fichier), `snapshots/<label>/`, et `history/<path>/<file>@v<N>` ainsi que `@latest`. Voir le [guide de montage](../MOUNT_GUIDE.md) (en vietnamien) pour plus de détails.

## Architecture

### Chemin d'écriture

```
input file
   │
   ▼
FastCDC chunker (16 / 64 / 256 KiB)
   │
   ▼
ChunkId = BLAKE3(plaintext chunk)
   │
   ├── already stored? ──► reuse the ID, write nothing
   │
   └── new ──► zstd (if it helps) ──► encrypt (if enabled) ──► append to segment
   │
   ▼
manifest (ordered ChunkIds and lengths)
   │
   ▼
WAL record appended and fsynced
   │
   ▼
redb metadata updated: name → ObjectId, ObjectId → versions → manifest
```

### Composants

- **Bloc (chunk) :** une plage d'octets immuable produite par FastCDC, identifiée par BLAKE3 et vérifiée par CRC32C.
- **Stockage par segments :** des fichiers en ajout seul (`segment_00000001.seg`, ...) qui changent de fichier vers 256 Mio. La lecture utilise un cache de descripteurs de fichiers.
- **Manifeste :** la liste ordonnée des blocs qui composent une version d'un fichier.
- **ObjectId :** un identifiant logique hybride de 128 bits qui reste le même après modification ou renommage, ce qui permet à l'historique de suivre le fichier.
- **Métadonnées :** stockées dans [redb](https://crates.io/crates/redb) : index des noms, enregistrements d'objets, manifestes, instantanés et état de la surveillance.
- **WAL :** journal de type redo uniquement. Un enregistrement ne contient que les données des blocs qui n'étaient pas encore stockés, ce qui évite d'écrire chaque bloc deux fois.

### Sûreté face aux plantages

- Les métadonnées ne sont mises à jour qu'une fois l'enregistrement du WAL rendu durable. La reprise rejoue les enregistrements validés de manière idempotente et vérifie que chaque bloc d'un manifeste est durable avant de le valider.
- La reprise ne tronque qu'une fin d'écriture incomplète dans le segment le plus récent. Toute corruption ailleurs est signalée comme une erreur et les données restent intactes.
- Le GC écrit les segments compactés dans un dossier temporaire, renomme les anciens segments en `.seg.old`, puis met les nouveaux en place. Si un plantage interrompt l'opération, le démarrage suivant restaure à partir des fichiers `.seg.old`.

## Chiffrement

```bash
# Créer un dépôt chiffré (phrase secrète d'au moins 8 caractères)
oos-lite --password-file ~/.oos-password init

# Toutes les commandes suivantes nécessitent la même phrase secrète
oos-lite --password-file ~/.oos-password put backup.tar.gz
export OOS_PASSWORD='...'   # alternative
```

Préférez `--password-file` (avec `chmod 600`) à `--password`, qui reste dans l'historique du shell.

Fonctionnement : une clé maître aléatoire de 256 bits chiffre le contenu des blocs et les enregistrements du WAL avec XChaCha20-Poly1305, avec un nonce aléatoire par enregistrement. La clé maître est enveloppée dans le fichier `vault.key` de 100 octets, à l'aide d'une clé dérivée de votre phrase secrète par Argon2id (64 Mio, 3 itérations). Un dépôt existant non chiffré ne peut pas passer en mode chiffré.

**Limites à connaître :**

- **Les métadonnées ne sont pas chiffrées.** Les noms de fichiers, tailles, horodatages, historiques de versions, libellés d'instantanés et manifestes de la base redb sont stockés en clair.
- **Les identifiants de blocs sont de simples hachages BLAKE3 du contenu.** Une personne ayant accès en lecture au dépôt peut savoir si un fichier qu'elle possède déjà s'y trouve, en le découpant en blocs et en comparant les hachages.
- **Perdre la phrase secrète ou `vault.key` revient à perdre les données.** Il n'existe aucun mécanisme de récupération.

Utilisez le chiffrement pour protéger le contenu des fichiers en cas de perte d'un disque ou de sauvegarde vers un emplacement non fiable, pas pour cacher quels fichiers vous possédez.

## Interface web et partage public

```bash
oos-lite ui --port 3000   # ouvre http://127.0.0.1:3000
oos-lite app              # même tableau de bord dans une fenêtre de bureau
```

Le tableau de bord comprend un explorateur de fichiers (vues liste, arborescence et grille), des aperçus, l'historique des versions, les instantanés, le téléversement (y compris de dossiers), le contrôle de la surveillance et GC/fsck.

Le serveur de l'interface écoute sur `127.0.0.1` par défaut et vérifie les en-têtes `Host`, `Origin` et `Sec-Fetch-Site` pour bloquer le DNS rebinding et les requêtes intersites. Il n'a **aucune authentification**. Si vous l'exposez sur `0.0.0.0`, n'importe qui sur votre réseau peut l'utiliser.

**Le partage public est activé par défaut.** Au démarrage de l'interface, OOS-Lite :

1. lance un serveur de partage sur le port `3001`, à l'écoute sur toutes les interfaces réseau ;
2. ouvre un Cloudflare quick tunnel vers ce port, en téléchargeant la dernière version de `cloudflared` dans le dossier du dépôt si elle n'y est pas déjà.

Les liens partagés sont protégés par un jeton aléatoire et une date d'expiration optionnelle. Si vous n'avez pas besoin du partage, désactivez-le :

```bash
OOS_DISABLE_PUBLIC_SHARING=1 oos-lite ui        # Linux / macOS
$env:OOS_DISABLE_PUBLIC_SHARING = "1"           # Windows PowerShell, puis lancez oos-lite ui
```

## Organisation sur disque

```
.oos-store/
├── store.lock              # exclusive process lock
├── vault.key               # only for encrypted stores
├── segments/               # append-only chunk segments
│   └── segment_00000001.seg
├── wal/
│   └── wal.log
├── metadata.db.open.lock
└── metadata.db/
    ├── metadata.redb       # active metadata database
    ├── .redb-active        # migration completed marker (do not remove)
    ├── .redb-migrating     # present while a migration is in progress
    ├── conf                # blocks older sled-based binaries from opening the store
    ├── conf.sled-backup    # original sled settings (migrated stores only)
    └── db, snap.*          # original sled data (migrated stores only)
```

Les versions antérieures à la migration vers redb stockaient les métadonnées dans sled. Lorsqu'une version récente ouvre un tel dépôt, elle importe les données sled via `metadata.redb.staging`, laisse les fichiers sled d'origine en place et installe un verrou qui empêche les anciens binaires d'ouvrir le dépôt migré. Une migration interrompue reprend automatiquement. Dès que `.redb-active` existe, une base absente ou endommagée est signalée comme une erreur au lieu d'être réimportée silencieusement depuis des données sled obsolètes.

Pour sauvegarder un dépôt, copiez le dossier entier et ne supprimez jamais les fichiers marqueurs d'état.

## Structure du projet

```
oos-lite/
├── core/                 # storage engine library (oos-lite-core)
│   ├── src/
│   │   ├── engine.rs     # StorageEngine: ties everything together
│   │   ├── chunk/        # FastCDC, ChunkId, chunk store
│   │   ├── segment/      # segment format, writer, reader, store
│   │   ├── manifest/     # manifests
│   │   ├── object/       # ObjectId and version records
│   │   ├── index/        # redb metadata and sled migration
│   │   ├── wal/          # write-ahead log
│   │   ├── snapshot/     # snapshots
│   │   ├── gc/           # garbage collection
│   │   ├── fsck/         # integrity checking
│   │   ├── crypto/       # vault key and chunk encryption
│   │   ├── vfs/          # virtual tree used by mounts
│   │   └── watcher/      # Auto-Vault folder watcher
│   └── tests/            # integration, crash-recovery, encryption, watcher tests
├── cli/                  # oos-lite and oos-lite-gui binaries
│   └── src/
│       ├── main.rs       # CLI (clap)
│       ├── gui_main.rs   # desktop app entry point
│       ├── ui/           # embedded web server and dashboard
│       ├── mount/        # WebDAV and FUSE
│       ├── share.rs      # public sharing and tunnel
│       ├── tray.rs       # Windows system tray
│       └── shell_ext.rs  # Windows Explorer integration
├── benchmarks/           # Criterion benchmarks and comparison runner
├── installer/            # Inno Setup script and icons
└── docs/                 # mount guide, support page, translations
```

## Développement

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets
cargo fmt --all
cargo bench --workspace                # benchmarks Criterion
cargo run -p oos-lite-benchmarks       # comparaison avec une copie classique
```

L'historique des versions se trouve dans [release_notes.md](../../release_notes.md) et sur la [page des versions](https://github.com/pudo58/oos-lite/releases).

## Contribuer

Les contributions sont les bienvenues. Lisez [CONTRIBUTING.fr.md](CONTRIBUTING.fr.md) avant d'ouvrir une issue ou une pull request. Les problèmes de sécurité doivent être signalés en privé, comme expliqué dans ce document.

Si OOS-Lite vous est utile, vous pouvez aussi [soutenir le projet](https://pudo58.github.io/oos-lite/support/).

## Licence

Double licence, au choix :

- Licence MIT ([LICENSE-MIT](../../LICENSE-MIT))
- Licence Apache, version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE))
