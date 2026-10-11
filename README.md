# croit AIplane

## Your organization's AI workspace, gateway and agent platform.

Put AI to work across your organization with one self-hosted platform. AIplane
combines the employee workspace, multi-provider model gateway and business
agents—and adds browser control through its dedicated extension. Administrators
connect company systems and define who can access models, tools and workflows.

[Read the manual](https://croit.github.io/aiplane/) · [Get started](docs/getting-started.md) ·
[Explore the user guide](docs/README.md#use-aiplane) · [Connect an application](docs/reference/api.md) ·
[Deploy AIplane](docs/operations/deployment.md)

[![Latest release](https://img.shields.io/github/v/release/croit/aiplane?label=Latest%20release)](https://github.com/croit/aiplane/releases/latest) [![License: AGPL-3.0-only](https://img.shields.io/badge/License-AGPL--3.0--only-blue.svg)](LICENSE)

![A conversation in croit AIplane, showing a model response, an approved tool call and the resulting answer. The conversation uses synthetic demo data.](docs/img/guide/chat-overview.png)

*One workspace for models, files and approved tools. Shown with synthetic demo data.*

## Key features

### A workspace your team can use every day

- 💬 **Persistent AI conversations.** Choose an allowed model, follow streaming
  replies, continue or redirect work, share conversations, fork them, and export
  to Markdown or PDF. [Explore chat](docs/guide/chat.md).
- 📎 **Files and versioned documents.** Bring files into a conversation, create
  editable canvas documents, review revisions, restore earlier versions, and
  download generated assets. [Explore files and canvas](docs/guide/files-and-canvas.md).
- 🎙️ **Voice and image workflows.** Dictate messages, use spoken conversations,
  generate images and edit them when the installation has the required models.
- 🧠 **Personal memory and reusable skills.** Keep preferences and project
  context available across conversations; add personal skills for repeatable
  instructions. [Explore memory](docs/guide/account-and-usage.md) · [Explore skills](docs/guide/tools-and-integrations.md).

### A gateway for your applications and models

- 🔀 **One model gateway across providers.** Connect hosted and self-hosted
  backends, organize models into pools, and choose how requests are distributed.
  [Connect models](docs/admin/models.md).
- 🩺 **Keep requests moving when a replica fails.** AIplane checks backend health,
  balances requests across available replicas and retries eligible failures on
  another replica before any model output reaches the client.
- 🎯 **Aliases and automatic routing.** Give applications stable model names and
  route requests among eligible models by quality, balance or cost.
  [Configure automatic routing](docs/admin/models.md#create-an-automatic-route).
- 🔌 **Familiar APIs for existing clients.** Connect OpenAI-compatible chat,
  embeddings, image and audio endpoints, or use the Anthropic Messages API;
  streaming is available where supported.
  [Use the API](docs/reference/api.md).
- 🧰 **Governed tools for applications as well as people.** Where supported,
  API clients can use server-side tools granted to their token and identity;
  AIplane runs approved tools in the gateway and returns the result to the model.
  [Connect applications](docs/reference/api.md#server-side-tools-and-client-owned-tools).
- 📈 **Live health and load awareness.** Inspect upstream health and in-flight
  requests; AIplane selects among healthy backends using the configured pool
  strategy.

### Tools and company knowledge, built into the workflow

- 🌐 **Browser control in the user's browser.** The dedicated AIplane extension
  lets an assistant navigate, read, click, type and capture pages in a browser
  paired with the conversation. [Set up browser control](docs/guide/tools-and-integrations.md#use-browser-control).
- 🔎 **Web search and page retrieval.** Find current information with configured
  search providers, then retrieve readable content from permitted web pages.
- 📚 **Search company knowledge.** Index and retrieve approved material from
  configured sources such as Git, WebDAV and Google Drive.
  [Set up knowledge collections](docs/admin/knowledge.md).
- 🧩 **Connect tools through MCP and skills.** Make approved external services
  and reusable instructions available to the people and agents who need them.
  [Configure integrations](docs/admin/integrations.md).

### Agents that can act—and know when to ask

- 🤖 **Build agents around real processes.** Define models, instructions,
  knowledge, tools, identity checks, state and routes in the agent builder.
  [Create an agent](docs/agent-guide/create.md).
- 🧪 **Test before publishing.** Run repeatable test cases against a draft or a
  published version, inspect the results and optionally require a passing suite
  before publication. [Test and publish](docs/agent-guide/test-publish.md).
- 🌍 **Publish agents where people need them.** Serve an agent in a website
  widget or expose it through the A2A interface.
  [Publish channels](docs/agent-guide/run-observe.md).
- 🤝 **Keep people in the loop.** Agents and tools can request approval, collect
  secure input or hand a task to a person; authorized colleagues respond in the
  shared inbox.
- ⏱️ **Automate recurring and event-driven work.** Schedule prompts or trigger
  runs with webhooks, then inspect the conversation and run history.
  [Explore schedules, webhooks and inbox](docs/guide/automation-and-inbox.md).

### Controls for administrators

- 🛡️ **Identity and group-based permissions.** Sign in through OIDC and grant
  access to models, tools, skills and agents by user group.
  [Manage access](docs/admin/access.md).
- 🔑 **Scoped tokens and usage limits.** Issue revocable API tokens, restrict
  model and tool access, set request, token or cost quotas, and review usage.
- ⚖️ **Compliance-aware model controls.** Declare GDPR and NDA coverage for
  model pools and configure a content guard to monitor, confirm or deny requests
  that target a pool without the declared coverage.
  [Review model controls](docs/admin/models.md) · [Configure the content guard](docs/admin/settings.md#access-and-content-guard).
- 📊 **Agent insights and activity history.** Inspect run analytics and export
  agent activity records for review.
  [Observe agent runs](docs/agent-guide/run-observe.md).

### Ready to deploy and operate

- 🐳 **Deploy on infrastructure you operate.** Choose Docker, Compose,
  Podman/systemd Quadlet or Kubernetes with Helm.
  [Compare deployment options](docs/operations/deployment.md).
- 🌐 **Use AIplane in six languages.** The interface ships in English, German,
  French, Spanish, Russian and Chinese.
- 📖 **Find the right docs in every installation.** Each build includes its
  version-matched manual, search, screenshots and Markdown exports for LLMs.
  [See the docs formats](docs/documentation-system.md).

![croit AIplane connects people and applications with model providers, tools and company knowledge.](docs/img/architecture.svg)

*People chat in the workspace, applications call the gateway, and agents use
configured tools and knowledge—with access controlled by the installation.*

Optional services and capabilities depend on configuration, provider support and
grants. The [manual](docs/README.md) explains prerequisites and controls for each
feature.

## Start with the job you need to do

| If you want to… | Start here |
|---|---|
| Set up AIplane for your organization | [Installation and first login](docs/getting-started.md) |
| Give employees access to models | [Models and routing](docs/admin/models.md) · [Access administration](docs/admin/access.md) |
| Ask questions about company documents | [Knowledge collections](docs/admin/knowledge.md) |
| Work with files and create editable documents | [Files and canvas](docs/guide/files-and-canvas.md) |
| Connect tools and accounts | [Tools and integrations](docs/guide/tools-and-integrations.md) |
| Create and publish an agent | [Create an agent](docs/agent-guide/create.md) · [Test and publish](docs/agent-guide/test-publish.md) |
| Connect an existing application | [HTTP API](docs/reference/api.md) |

## Try AIplane

AIplane needs an OpenID Connect identity provider and a reachable,
OpenAI-compatible model service. It does not include user-password sign-in or a
model server. Start the container with a persistent data volume and a session
key:

```bash
export AIPLANE_SESSION_KEY="$(openssl rand -hex 32)"
docker run -d --name aiplane \
  -p 127.0.0.1:8080:8080 \
  -e AIPLANE_SESSION_KEY \
  -v aiplane-data:/var/lib/gateway \
  ghcr.io/croit/aiplane:production
```

Open `http://localhost:8080` on the Docker host and follow the setup screen.
Register its `/auth/callback` address with your identity provider, sign in, and
connect a model backend under **Models → Upstreams**. Then open Chat, choose a
model and send a message.

Keep the data volume and session key across container replacements. The key
also derives the default encryption key for stored credentials, so keep it with
your backups. For remote access, put AIplane behind HTTPS. See the
[complete installation guide](docs/getting-started.md) and
[deployment options](docs/operations/deployment.md) for production setup,
optional services and recovery.

## Connect your applications

Use the `/v1` API to connect existing software. AIplane supports OpenAI chat
completions and other documented model endpoints, plus the Anthropic Messages
format. The available endpoints depend on configured providers and your access.

Create an API token in **Settings → API tokens** (`/settings/tokens`), then use
the model ID or alias exposed by your installation:

```python
from openai import OpenAI
import os

client = OpenAI(
    base_url="https://aiplane.example.com/v1",
    api_key=os.environ["AIPLANE_TOKEN"],
)
answer = client.chat.completions.create(
    model="your-model-id",
    messages=[{"role": "user", "content": "Hello"}],
)
print(answer.choices[0].message.content)
```

Applications can also use granted server-side tools where the endpoint supports
them. User and token restrictions still apply. See the
[API reference](docs/reference/api.md) for supported endpoints and limits.

## Documentation for every installation

The complete manual is available inside each packaged installation at **`/docs/`**,
including before first login. It includes local search, screenshots, and the
version and commit of the running build. The same Markdown pages are published
as the [documentation website](https://croit.github.io/aiplane/) and as formats
suitable for language models:

- `/docs/llms.txt` — an index of the manual.
- `/docs/llms-full.txt` — the complete manual in Markdown.
- `/docs/markdown/` — individual Markdown pages and their image assets.

## Deploy or build from source

Use [Docker Compose](deploy/compose.example.yml),
[Podman/systemd Quadlet](deploy/quadlet/aiplane.container), or
[Helm on Kubernetes](deploy/helm/aiplane/). The
[deployment guide](docs/operations/deployment.md) covers prerequisites, TLS,
persistence and optional services.

`/healthz` and `/readyz` serve liveness and setup readiness. A Prometheus
scrape endpoint, `GET /metrics`, is off by default: switch it on with a scrape
token, an allowed IP list or both under **Administration → Settings → Access**.
Behind a reverse proxy the IP list needs `AIPLANE_TRUSTED_PROXIES` set to the
proxy. See [monitoring](docs/operations/monitoring.md).

To build from source, install [mise](https://mise.jdx.dev/) and run:

```bash
mise install
mise run build
```

This builds the release binary and the web UI and manual. The frontend/manual
artifact is `target/frontend/build/`; ship it alongside the binary and set
`AIPLANE_STATIC_DIR` to its installed location. The runtime image copies this
artifact and does not need Python or Node to serve the application.

For development, use `mise run dev` for the web UI and gateway, or `mise run
dev-ui` for the isolated browser fixture. See the
[developer workflow](docs/dev-workflow.md).

## Contributing and support

Report reproducible problems through [GitHub Issues](https://github.com/croit/aiplane/issues).
Include the build version, steps to reproduce, expected behaviour and a
redacted error. Keep credentials and private content out of reports. Before
changing behaviour, read the [testing strategy](docs/testing.md).

## License

GNU Affero General Public License v3.0 only. See [LICENSE](LICENSE).
