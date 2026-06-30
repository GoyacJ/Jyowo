use super::*;

pub trait WorkspaceCreateRequest {
    type Output;

    fn create_workspace(self, harness: &Harness) -> Result<Self::Output, HarnessError>;
}

impl Harness {
    pub async fn create_workspace<R>(&self, request: R) -> Result<R::Output, HarnessError>
    where
        R: WorkspaceCreateRequest,
    {
        request.create_workspace(self)
    }

    pub async fn list_workspaces(&self, tenant: TenantId) -> Result<Vec<Workspace>, HarnessError> {
        if tenant != self.inner.options.tenant_policy.id
            && !self.inner.options.tenant_policy.allow_scoped_tenants
        {
            return Err(HarnessError::InvalidTenant(tenant));
        }
        Ok(self.inner.workspace_registry.list(tenant))
    }

    pub async fn get_workspace(
        &self,
        id: harness_contracts::WorkspaceId,
    ) -> Result<Option<Workspace>, HarnessError> {
        let workspace = self.inner.workspace_registry.get(id);
        if let Some(workspace) = &workspace {
            if workspace.tenant_id != self.inner.options.tenant_policy.id
                && !self.inner.options.tenant_policy.allow_scoped_tenants
            {
                return Err(HarnessError::InvalidTenant(workspace.tenant_id));
            }
        }
        Ok(workspace)
    }

    fn create_workspace_path(&self, root: &Path) -> Result<PathBuf, HarnessError> {
        std::fs::create_dir_all(root)
            .map_err(|error| HarnessError::Other(format!("create workspace failed: {error}")))?;
        for relative in GOVERNED_WORKSPACE_DIRS {
            std::fs::create_dir_all(root.join(relative)).map_err(|error| {
                HarnessError::Other(format!("create workspace path {relative} failed: {error}"))
            })?;
        }
        root.canonicalize()
            .map_err(|error| HarnessError::Other(format!("canonicalize workspace failed: {error}")))
    }

    fn create_workspace_record(&self, mut spec: WorkspaceSpec) -> Result<Workspace, HarnessError> {
        if spec.tenant_id != self.inner.options.tenant_policy.id
            && !self.inner.options.tenant_policy.allow_scoped_tenants
        {
            return Err(HarnessError::InvalidTenant(spec.tenant_id));
        }
        spec.root_path = self.create_workspace_path(&spec.root_path)?;
        Ok(self.inner.workspace_registry.create(spec))
    }
}

const GOVERNED_WORKSPACE_DIRS: &[&str] = &[
    "config",
    "data",
    "runtime/events",
    "runtime/sessions",
    "logs",
    "tmp",
];

impl WorkspaceCreateRequest for WorkspaceSpec {
    type Output = Workspace;

    fn create_workspace(self, harness: &Harness) -> Result<Self::Output, HarnessError> {
        harness.create_workspace_record(self)
    }
}

impl WorkspaceCreateRequest for PathBuf {
    type Output = PathBuf;

    fn create_workspace(self, harness: &Harness) -> Result<Self::Output, HarnessError> {
        harness.create_workspace_path(&self)
    }
}

impl WorkspaceCreateRequest for &PathBuf {
    type Output = PathBuf;

    fn create_workspace(self, harness: &Harness) -> Result<Self::Output, HarnessError> {
        harness.create_workspace_path(self)
    }
}

impl WorkspaceCreateRequest for &Path {
    type Output = PathBuf;

    fn create_workspace(self, harness: &Harness) -> Result<Self::Output, HarnessError> {
        harness.create_workspace_path(self)
    }
}
