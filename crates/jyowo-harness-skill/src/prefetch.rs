use crate::{SkillError, SkillLoader, SkillRegistry};

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub enum SkillPrefetchStrategy {
    Disabled,
    #[default]
    Eager,
    LazyPerTurn {
        concurrency: usize,
    },
    HintDriven,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct SkillPrefetchPlan {
    pub load_on_session_start: bool,
    pub load_limit: Option<usize>,
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub struct SkillPrefetchReport {
    pub loaded: usize,
    pub rejected: usize,
}

#[derive(Clone)]
pub struct SkillPrefetcher {
    loader: SkillLoader,
    registry: SkillRegistry,
    strategy: SkillPrefetchStrategy,
}

impl SkillPrefetchStrategy {
    #[must_use]
    pub fn plan_for_skill_count(self, skill_count: usize) -> SkillPrefetchPlan {
        match self {
            Self::Disabled => SkillPrefetchPlan {
                load_on_session_start: false,
                load_limit: Some(0),
            },
            Self::Eager => SkillPrefetchPlan {
                load_on_session_start: true,
                load_limit: None,
            },
            Self::LazyPerTurn { concurrency } => SkillPrefetchPlan {
                load_on_session_start: skill_count <= concurrency,
                load_limit: Some(concurrency),
            },
            Self::HintDriven => SkillPrefetchPlan {
                load_on_session_start: false,
                load_limit: None,
            },
        }
    }
}

impl SkillPrefetcher {
    #[must_use]
    pub fn new(
        loader: SkillLoader,
        registry: SkillRegistry,
        strategy: SkillPrefetchStrategy,
    ) -> Self {
        Self {
            loader,
            registry,
            strategy,
        }
    }

    pub async fn prefetch_all(&self) -> Result<SkillPrefetchReport, SkillError> {
        match self.strategy {
            SkillPrefetchStrategy::Disabled | SkillPrefetchStrategy::HintDriven => {
                Ok(SkillPrefetchReport::default())
            }
            SkillPrefetchStrategy::Eager => self.load_matching(None, None).await,
            SkillPrefetchStrategy::LazyPerTurn { concurrency } => {
                self.load_matching(None, Some(concurrency)).await
            }
        }
    }

    pub async fn prefetch_hints(
        &self,
        hints: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Result<SkillPrefetchReport, SkillError> {
        let hints = hints
            .into_iter()
            .map(|hint| hint.as_ref().to_owned())
            .collect::<Vec<_>>();
        match self.strategy {
            SkillPrefetchStrategy::Disabled => Ok(SkillPrefetchReport::default()),
            SkillPrefetchStrategy::HintDriven => self.load_matching(Some(&hints), None).await,
            SkillPrefetchStrategy::Eager => self.prefetch_all().await,
            SkillPrefetchStrategy::LazyPerTurn { concurrency } => {
                self.load_matching(Some(&hints), Some(concurrency)).await
            }
        }
    }

    async fn load_matching(
        &self,
        hints: Option<&[String]>,
        limit: Option<usize>,
    ) -> Result<SkillPrefetchReport, SkillError> {
        let report = self.loader.load_prefetch_batch(hints, limit).await?;
        let mut loaded = 0;
        let mut rejected = report.rejected.len();

        for skill in report.loaded {
            match self.registry.register(skill) {
                Ok(()) => loaded += 1,
                Err(SkillError::Duplicate(_)) => rejected += 1,
                Err(error) => return Err(error),
            }
        }

        Ok(SkillPrefetchReport { loaded, rejected })
    }
}
