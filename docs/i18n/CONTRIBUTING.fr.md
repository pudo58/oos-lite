# Contribuer à OOS-Lite

[English](../../CONTRIBUTING.md) | [简体中文](CONTRIBUTING.zh-CN.md) | [日本語](CONTRIBUTING.ja.md) | **Français** | [한국어](CONTRIBUTING.ko.md) | [Tiếng Việt](CONTRIBUTING.vi.md)

> Ce document est une traduction du [CONTRIBUTING anglais](../../CONTRIBUTING.md). En cas de divergence, la version anglaise fait foi.

Merci de votre intérêt pour OOS-Lite. Les rapports de bogues, corrections de documentation, tests et contributions de code sont tous les bienvenus. Ce guide explique comment préparer votre environnement et à quoi ressemble une bonne contribution.

## Comment contribuer

- **Signaler un bogue.** Ouvrez une issue en indiquant votre système, la version d'OOS-Lite (`oos-lite --version`), les commandes exécutées, le résultat attendu et le résultat obtenu. Les journaux sont utiles ; lancez avec `RUST_LOG=debug` pour plus de détails.
- **Proposer une fonctionnalité.** Ouvrez une issue décrivant le problème à résoudre avant d'écrire du code, surtout pour les changements du format sur disque ou de la ligne de commande.
- **Améliorer la documentation ou les traductions.** Le README et le CONTRIBUTING existent en anglais, chinois, japonais, français, coréen et vietnamien. Les fichiers anglais font référence ; les traductions se trouvent dans [`docs/i18n/`](.).
- **Envoyer une pull request.** Les PR petites et ciblées sont relues plus vite que les grosses.

## Signaler un problème de sécurité

N'ouvrez **pas** d'issue publique pour un problème de sécurité, par exemple une fuite de données via l'interface web ou le serveur de partage, une faiblesse du chiffrement ou une traversée de chemin. Utilisez plutôt le [signalement privé de vulnérabilité](https://github.com/pudo58/oos-lite/security/advisories/new) de GitHub et indiquez les étapes de reproduction. Laissez au mainteneur un délai raisonnable pour publier un correctif avant toute divulgation publique.

## Environnement de développement

Il vous faut :

- une chaîne d'outils Rust stable récente avec `clippy` et `rustfmt` (`rustup component add clippy rustfmt`) ;
- un éditeur de liens C (MSVC ou MinGW sous Windows) ;
- sous Linux, `libfuse3-dev` et `pkg-config` pour compiler la prise en charge de FUSE.

```bash
git clone https://github.com/<your-username>/oos-lite.git
cd oos-lite
cargo build --workspace
cargo test --workspace
```

L'espace de travail contient trois crates :

| Crate | Chemin | Contenu |
| :--- | :--- | :--- |
| `oos-lite-core` | `core/` | Moteur de stockage : blocs, segments, WAL, métadonnées, GC, chiffrement, surveillance |
| `oos-lite` | `cli/` | Ligne de commande, application de bureau, interface web, montage, partage, intégration Windows |
| `oos-lite-benchmarks` | `benchmarks/` | Benchmarks Criterion et outil de comparaison |

Pour les tests manuels, utilisez toujours un dépôt jetable, par exemple `oos-lite --store-dir /tmp/oos-test ...`, afin de ne jamais toucher à de vraies données. Définissez `OOS_DISABLE_PUBLIC_SHARING=1` lorsque vous lancez l'interface en local si vous n'avez pas besoin du partage.

## Avant d'ouvrir une pull request

Exécutez ces commandes à la racine du dépôt :

```bash
cargo fmt --all
cargo clippy --workspace --all-targets
cargo test --workspace
```

La CI exécute `cargo check` et `cargo test` sous Ubuntu et Windows. Vérifiez aussi que votre modification n'ajoute pas de nouveaux avertissements clippy.

### Modifications du moteur de stockage

`core/` contient les données des utilisateurs ; les modifications demandent donc une attention particulière :

- **Ajoutez des tests.** Une correction de bogue nécessite un test de non-régression (voir `core/tests/core_regressions.rs`). Les changements des chemins d'écriture, de reprise ou de GC doivent être couverts par les tests de plantage de `core/tests/kill_recovery.rs`.
- **Préservez la compatibilité du format sur disque.** Les dépôts, segments, enregistrements WAL et métadonnées existants doivent continuer à fonctionner. Si un changement de format est inévitable, discutez-en d'abord dans une issue et fournissez un chemin de migration avec des tests, comme la migration de sled vers redb dans `core/src/index/migration.rs`.
- **Respectez l'ordre de durabilité.** Les données doivent être durables (fsync) avant que les métadonnées qui y font référence soient publiées.
- **Ne journalisez pas de secrets.** Les phrases secrètes et les clés ne doivent jamais apparaître dans les journaux, les messages d'erreur ou la sortie `Debug`.
- **Évitez `unwrap()` et `expect()`** dans le code hors tests qui traite des entrées utilisateur ou des données sur disque. Renvoyez plutôt une `OosLiteError`.

### Modifications de la ligne de commande, de l'interface ou du réseau

- Gardez le serveur de l'interface lié à `127.0.0.1` par défaut et conservez les vérifications Host/Origin.
- Tout ce qui écoute sur le réseau, ou télécharge et exécute du code, doit être clairement expliqué dans la description de la PR.
- Si vous modifiez des commandes ou des options, mettez à jour le README (au minimum la version anglaise).

## Messages de commit

Utilisez [Conventional Commits](https://www.conventionalcommits.org/), comme dans l'historique existant :

```
feat(core): add keyed chunk IDs for encrypted stores
fix(watcher): retry files locked during startup
docs: update mount guide
```

Types courants : `feat`, `fix`, `docs`, `test`, `refactor`, `perf`, `ci`, `chore`. Limitez chaque commit à un seul changement ; évitez de regrouper plusieurs fonctionnalités dans un même commit.

## Processus de pull request

1. Forkez le dépôt et créez une branche à partir de `master`.
2. Apportez votre modification, avec tests et documentation.
3. Lancez le formatage, clippy et les tests comme indiqué ci-dessus.
4. Ouvrez une pull request qui explique ce qui a changé, pourquoi, et comment vous l'avez testé. Liez l'issue correspondante s'il y en a une.
5. Répondez aux retours de relecture en poussant de nouveaux commits sur la même branche.

## Traductions

Lorsque vous modifiez `README.md` ou `CONTRIBUTING.md`, vous n'avez pas à mettre à jour toutes les traductions vous-même. Indiquez dans la PR que les traductions sont obsolètes, et des locuteurs natifs pourront prendre le relais. Si vous mettez à jour une traduction, gardez les blocs de code, commandes et liens identiques à la version anglaise.

## Licence

En contribuant, vous acceptez que vos contributions soient publiées sous la double licence MIT et Apache 2.0, comme le projet, sans conditions supplémentaires.
