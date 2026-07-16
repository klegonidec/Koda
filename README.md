# duo-bridge

MVP Rust/Axum pour recevoir des webhooks Jira/GitLab, mettre en file des sessions et piloter OpenCode sur une instance EC2.

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
docker buildx build --platform linux/amd64,linux/arm64 -t duo-bridge:dev --push .
```

## Webhooks

- Jira : `POST /api/v1/webhooks/jira/work-items`
- GitLab code : `POST /api/v1/webhooks/gitlab/code-events`
- Déclenchement pipeline : `POST /api/v1/webhooks/gitlab/pipeline-trigger`

Le MVP doit être placé derrière HTTPS. Les secrets de production doivent être injectés via fichiers secrets (`*_FILE`) ou un gestionnaire de secrets, jamais committés.

## État actuel

Le socle exécutable comprend le setup/admin, SQLite, sessions persistées, queue de jobs, dashboard, skills et endpoints webhook. L’adaptateur OpenCode effectue le health-check ; l’envoi de prompts et le clonage GitLab sont les prochaines étapes du lot d’orchestration.
