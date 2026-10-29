import { request } from './api.ts';
import type { AgentSummary } from './agents.ts';

export interface Organization { id: string; name: string; description: string; revision: string }
export interface AdminOrganization extends Organization { member_groups: string[] }
export interface OrganizationListRow extends Organization { can_create_agent: boolean }
export interface OrganizationDetail { organization: Organization; agents: AgentSummary[]; active_count: number; active_limit: number; can_create_agent: boolean }
export interface OrganizationBody { name: string; description: string; member_groups: string[] }
export interface OrganizationEdit extends OrganizationBody { expected_revision: string }

export function organizationDraft(row?: AdminOrganization): OrganizationBody {
    return { name: row?.name ?? '', description: row?.description ?? '', member_groups: [...(row?.member_groups ?? [])] };
}

export function organizationChange(row: AdminOrganization, draft: OrganizationBody): OrganizationEdit | null {
    if (row.name === draft.name && row.description === draft.description && JSON.stringify([...row.member_groups].sort()) === JSON.stringify([...draft.member_groups].sort())) return null;
    return { ...draft, member_groups: [...draft.member_groups], expected_revision: row.revision };
}

const json = (method: string, body: unknown): RequestInit => ({ method, body: JSON.stringify(body) });
const path = (id: string) => `/api/v0/organizations/${encodeURIComponent(id)}`;
export const adminOrganizationsApi = {
    list: () => request<{ organizations: AdminOrganization[]; groups: string[]; member_group_user_counts: Record<string, number> }>('/api/v0/admin/organizations'),
    create: (body: OrganizationBody) => request<{ organization: AdminOrganization }>('/api/v0/admin/organizations', json('POST', body)),
    save: (id: string, body: OrganizationEdit) => request<{ organization: AdminOrganization }>(`/api/v0/admin/organizations/${encodeURIComponent(id)}`, json('PUT', body))
};
export const organizationsApi = {
    list: () => request<{ organizations: OrganizationListRow[] }>('/api/v0/organizations'),
    detail: (id: string) => request<OrganizationDetail>(path(id)),
    createAgent: (id: string, body: { name: string; display?: string; description: string; copy_from?: string }) => request<{ agent: AgentSummary }>(`${path(id)}/agents`, json('POST', body)),
    setActive: (id: string, agentId: string, active: boolean) => request<{ agent: AgentSummary }>(`${path(id)}/agents/${encodeURIComponent(agentId)}/active`, json('POST', { active }))
};
