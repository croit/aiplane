# Get started with croit AIplane

AIplane connects your users and applications to models, tools, agents and
company knowledge. Start by connecting one model and making one successful
chat request, then enable the capabilities your team needs.

## What you need

- Docker for the container path below, or one of the other
  [deployment methods](operations/deployment.md).
- An OpenID Connect identity provider, its issuer URL, and a registered client
  with a client ID and client secret. Setup requires a real sign-in.
- A reachable model server exposing an OpenAI-compatible API, or a hosted
  OpenAI-compatible service. AIplane does not include model weights or an
  inference server.
- Persistent storage and a safely stored session key.

For a publicly accessible deployment, configure HTTPS and the public hostname
before registering the identity provider callback. The exact callback is
`<public-url>/auth/callback`.

## Start the container

Generate a session key once and store it in your secret manager. Set
`AIPLANE_SESSION_KEY` in the shell from that stored value, then run:

```bash
docker run -d --name aiplane \
  -p 127.0.0.1:8080:8080 \
  -e AIPLANE_SESSION_KEY \
  -v aiplane-data:/var/lib/gateway \
  ghcr.io/croit/aiplane:production
```

The key is 64 hexadecimal characters (32 bytes). You can generate one with
`openssl rand -hex 32`. Preserve it with your backups: it signs browser
sessions and, unless you supply a separate encryption key, derives the key
used to encrypt stored secrets.

Open `http://localhost:8080` on the Docker host. A new installation opens the
setup wizard. For remote access, use your configured HTTPS reverse proxy.
The manual is available at `/docs/`, including before setup is complete.

## Complete setup

1. Enter the public URL, issuer URL, client ID and client secret in the wizard.
   The issuer must be an `http` or `https` URL; link-local and cloud-metadata
   addresses are refused, because the unauthenticated first-run wizard would
   otherwise be a generic server-side request probe.
2. Register the displayed callback URI with your identity provider.
3. Continue to the provider and sign in. AIplane verifies the login before
   saving the provider configuration.
4. Select the verified claim value that should grant administrator access.
5. Finish setup. Configure additional users and group access in the access
   administration pages.

Setup needs a working identity provider. It does not offer a local password
account. If sign-in fails, check the callback, issuer, client credentials and
the provider's client configuration before repeating setup.

## Connect a model

Open **Models** in the administration navigation. On its **Upstreams** tab,
configure the provider/backend connection and its pool. Use the actual
OpenAI-compatible base URL and the provider's API key if required. The model
server must be reachable from the AIplane process or container; `localhost`
inside a container refers to that container.

Follow [model and upstream administration](admin/models.md) for
the individual controls, health checks, model discovery, aliases and access.
Then open Chat and select a model you have permission to use. Send a short
message and check that the reply completes.

## Make your first API request

Open **Settings → API tokens** (`/settings/tokens`), create a token, and copy
its secret when it is displayed. Keep it in your application's secret store.
The token belongs to your user and can restrict model and tool access further.

Set `AIPLANE_TOKEN` to the token. Use an actual model ID or alias listed by
your installation. List accessible models first:

```bash
curl http://localhost:8080/v1/models \
  -H "Authorization: Bearer $AIPLANE_TOKEN"
```

Send a completion, replacing `your-model-id` with that ID:

```bash
curl http://localhost:8080/v1/chat/completions \
  -H "Authorization: Bearer $AIPLANE_TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"model":"your-model-id","messages":[{"role":"user","content":"Hello"}]}'
```

Use the HTTPS hostname in these requests when connecting remotely. Existing
OpenAI-compatible clients use that hostname followed by `/v1` as their base
URL. See the [API reference](reference/api.md) for the other protocols,
streaming, authentication and errors.

## Choose your next task

- [Use chat](guide/chat.md) and work with files and documents.
- [Configure tools and integrations](guide/tools-and-integrations.md).
- Create and publish an agent using the agent guide.
- Configure company knowledge using the administration guide.
- [Prepare production deployment](operations/deployment.md) and
  [backups](operations/backup-recovery.md).
