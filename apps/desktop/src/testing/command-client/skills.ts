import type {
  GetSkillCatalogEntryResponse,
  GetSkillCatalogFileResponse,
  GetSkillDetailResponse,
  GetSkillFileResponse,
  ListSkillCatalogEntriesResponse,
  ListSkillCatalogInstallTasksResponse,
  ListSkillCatalogSourcesResponse,
  ListSkillsResponse,
  SkillSummary,
} from '@/shared/tauri/commands'

export const fixtureWorkspaceSkill: SkillSummary = {
  description: 'Creates release notes from recent changes.',
  enabled: true,
  id: 'skill-001',
  importedAt: '2026-06-21T00:00:00.000Z',
  manageable: true,
  name: 'release-notes',
  sourceKind: 'workspace',
  status: 'ready',
  tags: ['writing'],
  updatedAt: '2026-06-21T00:00:00.000Z',
}

const fixtureBundledSkill: SkillSummary = {
  description: 'Inspects source changes and returns risks.',
  enabled: true,
  id: 'code-review',
  manageable: false,
  name: 'code-review',
  sourceKind: 'bundled',
  status: 'ready',
  tags: ['review'],
}

export const fixtureListSkills: ListSkillsResponse = {
  skills: [fixtureWorkspaceSkill, fixtureBundledSkill],
}

export const fixtureSkillCatalogSources: ListSkillCatalogSourcesResponse = {
  sources: [
    {
      description: 'Official Anthropic skills repository.',
      id: 'anthropic',
      installable: true,
      label: 'Anthropic Skills',
      trustLevel: 'official',
    },
    {
      description: 'Validation standard for portable agent skills.',
      id: 'agent-skills-spec',
      installable: false,
      label: 'Agent Skills spec',
      trustLevel: 'standard',
    },
    {
      description: 'Curated community index of agent skill repositories.',
      id: 'awesome-agent-skills',
      installable: true,
      label: 'Awesome Agent Skills',
      trustLevel: 'curated',
    },
    {
      description: 'Public ClawHub registry with security scan metadata.',
      id: 'clawhub',
      installable: true,
      label: 'ClawHub',
      trustLevel: 'community',
    },
  ],
}

export const fixtureSkillCatalogEntries: ListSkillCatalogEntriesResponse = {
  entries: [
    {
      description: 'Create distinctive frontend interfaces.',
      entryId: 'anthropic:frontend-design',
      homepageUrl: 'https://github.com/anthropics/skills/tree/main/frontend-design',
      installable: true,
      installed: false,
      name: 'frontend-design',
      sourceId: 'anthropic',
      sourceLabel: 'Anthropic Skills',
      tags: ['frontend'],
      trustLevel: 'official',
      version: 'main',
    },
  ],
}

export const fixtureSkillCatalogEntry: GetSkillCatalogEntryResponse = {
  entry: fixtureSkillCatalogEntries.entries[0],
  files: [{ kind: 'file', path: 'SKILL.md', sizeBytes: 512 }],
  readmePreview: 'Create distinctive frontend interfaces.',
  validation: {
    issues: [],
    status: 'ready',
  },
}

export const fixtureSkillDetail: GetSkillDetailResponse = {
  skill: {
    bodyPreview: 'Write concise release notes from the current workspace diff.',
    configKeys: ['CHANGELOG_TOKEN'],
    files: [
      {
        depth: 0,
        kind: 'file',
        name: 'SKILL.md',
        path: 'SKILL.md',
        sizeBytes: 96,
      },
      {
        depth: 0,
        kind: 'directory',
        name: 'references',
        path: 'references',
      },
      {
        depth: 1,
        kind: 'file',
        name: 'style.md',
        path: 'references/style.md',
        sizeBytes: 42,
      },
    ],
    parameters: [
      {
        description: 'Target release version.',
        name: 'version',
        paramType: 'string',
        required: true,
      },
    ],
    prerequisites: {
      missingConfigKeys: [],
      missingEnvVars: [],
    },
    scripts: [],
    summary: fixtureWorkspaceSkill,
  },
}

export const fixtureSkillEntryFile: GetSkillFileResponse = {
  file: {
    content: 'Write concise release notes from the current workspace diff.',
    path: 'SKILL.md',
  },
}

export const fixtureSkillCatalogFile: GetSkillCatalogFileResponse = {
  file: {
    content: 'Write concise release notes from the current workspace diff.',
    path: 'SKILL.md',
    truncated: false,
  },
}

export const fixtureSkillCatalogInstallTasks: ListSkillCatalogInstallTasksResponse = {
  tasks: [],
}
