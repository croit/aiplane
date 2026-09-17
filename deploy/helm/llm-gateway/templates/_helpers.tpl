{{/*
SPDX-License-Identifier: AGPL-3.0-only
Copyright (C) 2026 croit GmbH
*/}}

{{- define "llm-gateway.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "llm-gateway.fullname" -}}
{{- if .Values.fullnameOverride -}}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- $name := default .Chart.Name .Values.nameOverride -}}
{{- if contains $name .Release.Name -}}
{{- .Release.Name | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" -}}
{{- end -}}
{{- end -}}
{{- end -}}

{{- define "llm-gateway.labels" -}}
helm.sh/chart: {{ printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{ include "llm-gateway.selectorLabels" . }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end -}}

{{- define "llm-gateway.selectorLabels" -}}
app.kubernetes.io/name: {{ include "llm-gateway.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end -}}

{{- define "llm-gateway.serviceAccountName" -}}
{{- if .Values.serviceAccount.create -}}
{{- default (include "llm-gateway.fullname" .) .Values.serviceAccount.name -}}
{{- else -}}
{{- default "default" .Values.serviceAccount.name -}}
{{- end -}}
{{- end -}}

{{/*
Name of the Secret holding GATEWAY_SESSION_KEY — either the one the operator
brought, or the one this chart manages.
*/}}
{{- define "llm-gateway.sessionSecretName" -}}
{{- if .Values.sessionKey.existingSecret -}}
{{- .Values.sessionKey.existingSecret -}}
{{- else -}}
{{- printf "%s-session" (include "llm-gateway.fullname" .) -}}
{{- end -}}
{{- end -}}

{{/*
The session key itself, for the Secret this chart renders.

The gateway hex-decodes it and refuses to boot unless it is exactly 64 hex
chars / 32 bytes (`load_session_secret` in the binary), so a generated key is
the sha256 of a random string — sprig has no hex generator, and that is
precisely 64 lowercase hex chars.

Precedence: an explicit value wins; otherwise we re-read the key already in the
cluster so an upgrade never rotates it; only a genuinely fresh install
generates one. `lookup` returns an empty map when there is no cluster to ask
(`helm template`, `--dry-run`), which is exactly the GitOps trap called out in
values.yaml — hence the guard in secret.yaml that refuses to render a random
key when autoGenerate is off.
*/}}
{{- define "llm-gateway.sessionKey" -}}
{{- if .Values.sessionKey.value -}}
{{- if not (regexMatch "^[0-9a-fA-F]{64}$" .Values.sessionKey.value) -}}
{{- fail "sessionKey.value must be 64 hex chars (32 bytes) — generate one with `openssl rand -hex 32`" -}}
{{- end -}}
{{- .Values.sessionKey.value -}}
{{- else -}}
{{- $name := printf "%s-session" (include "llm-gateway.fullname" .) -}}
{{- $existing := lookup "v1" "Secret" .Release.Namespace $name -}}
{{- if and $existing $existing.data (index ($existing.data | default dict) "GATEWAY_SESSION_KEY") -}}
{{- index $existing.data "GATEWAY_SESSION_KEY" | b64dec -}}
{{- else -}}
{{- randAlphaNum 64 | sha256sum -}}
{{- end -}}
{{- end -}}
{{- end -}}

{{/*
Public URL: what the operator set, else the ingress host, else empty (the setup
wizard then derives it from the request).
*/}}
{{- define "llm-gateway.publicUrl" -}}
{{- if .Values.publicUrl -}}
{{- .Values.publicUrl -}}
{{- else if and .Values.ingress.enabled .Values.ingress.host -}}
{{- printf "%s://%s" (ternary "https" "http" .Values.ingress.tls.enabled) .Values.ingress.host -}}
{{- end -}}
{{- end -}}

{{/*
Which tag the images that ship with this chart (the gateway and the OCR
sidecar) are pulled by.

Three cases, in order:

  * an explicit `image.tag` always wins;
  * a chart installed from a git checkout has `appVersion: latest` and follows
    the `:latest` images — that IS the rolling channel for an unreleased state;
  * a released chart follows `:production` (the moving "latest official
    release" pointer) while auto-updates are on, and pins `v<appVersion>` — the
    images built from this chart's own commit — when they are off.

So the default is "stay current", and pinning is one value away.
*/}}
{{- define "llm-gateway.imageTag" -}}
{{- if .Values.image.tag -}}
{{ .Values.image.tag }}
{{- else if not (regexMatch "^[0-9]" .Chart.AppVersion) -}}
{{ .Chart.AppVersion }}
{{- else if .Values.autoUpdate.enabled -}}
production
{{- else -}}
v{{ .Chart.AppVersion }}
{{- end -}}
{{- end -}}

{{/*
Pull policy. A moving tag is worthless with `IfNotPresent` — the node would
keep serving whatever it cached the first time — so the two decisions are made
together rather than left to line up by hand.
*/}}
{{- define "llm-gateway.imagePullPolicy" -}}
{{- if .Values.image.pullPolicy -}}
{{ .Values.image.pullPolicy }}
{{- else if regexMatch "^(latest|production)$" (include "llm-gateway.imageTag" .) -}}
Always
{{- else -}}
IfNotPresent
{{- end -}}
{{- end -}}
