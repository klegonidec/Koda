# Koda

Control plane Rust/Axum pour exécuter OpenCode de manière contrôlée depuis Jira et GitLab sur une instance EC2.

Le MVP couvre trois parcours : Jira vers une Draft MR contrôlée, revue de merge request GitLab, et diagnostic de pipeline GitLab échoué. Koda conserve les preuves d’exécution, applique une politique par projet et ne merge jamais automatiquement.

## Démarrage local

```bash
cp .env.example .env
mkdir -p data
cargo run
```

Ouvrir `http://localhost:8080/setup`. Le mot de passe de bootstrap est `APP_SETUP_PASSWORD`; le mot de passe du compte admin doit contenir au moins 14 caractères.

## Docker

Le fichier `compose.yaml` utilise deux services : l’API Rust et un serveur OpenCode privé. Définir un `OPENCODE_VERSION` testé avant le build ; ne jamais utiliser `latest` en production.

```bash
docker compose build
docker compose up -d
```

Pour construire les deux architectures :

```bash
docker buildx build --platform linux/amd64,linux/arm64 -t koda:dev --push .
```

## Webhooks

- Jira : `POST /api/v1/webhooks/jira/work-items`
- GitLab MR : `POST /api/v1/webhooks/gitlab/code-events`
- GitLab pipeline échoué : `POST /api/v1/webhooks/gitlab/pipeline-events`

Le MVP doit être placé derrière HTTPS. Les secrets de production doivent être injectés via fichiers secrets (`*_FILE`) ou un gestionnaire de secrets, jamais committés.

## GitHub Actions

La CI GitHub valide le formatage, Clippy, les tests et un build Docker amd64 sur chaque push/PR. Un tag `vX.Y.Z` publie les images multiarchitecture dans GHCR. Définir la variable de dépôt `OPENCODE_VERSION` avec une version OpenCode testée avant de créer un tag.

## État actuel

Le socle comprend le setup/admin, SQLite, sessions persistées, politiques, preuves, approbations, dashboard React et endpoints des trois parcours. L’exécution OpenCode est encore encapsulée derrière l’adaptateur ; le runner éphémère et le proxy egress sont les prochaines étapes de durcissement.
