use super::*;

const PLUGIN_FAILURE_WITHHELD_MESSAGE: &str = "Plugin failure withheld from conversation timeline.";
const PLUGIN_DISCOVERY_FAILED_MESSAGE: &str = "Plugin discovery failed. See Activity for details.";
const PLUGIN_ACTIVATION_FAILED_MESSAGE: &str =
    "Plugin activation failed. See Activity for details.";
const PLUGIN_EVENT_DETAILS_WITHHELD: &str = "withheld";
const PLUGIN_EVENT_LOCAL_ORIGIN: &str = "<local-plugin>";
const PLUGIN_EVENT_CARGO_EXTENSION_ORIGIN: &str = "<cargo-extension>";
const PLUGIN_EVENT_REMOTE_ORIGIN: &str = "<remote-plugin>";

impl Harness {
    pub(super) async fn activate_plugins(
        &self,
        options: &SessionOptions,
    ) -> Result<(), HarnessError> {
        let Some(registry) = &self.inner.plugin_registry else {
            return Ok(());
        };
        let pending_plugin_events = Arc::new(PendingSessionEvents::default());
        let discovery_registry =
            registry.with_scoped_event_sink(Arc::new(BufferedPluginEventSink {
                pending_session_events: Arc::clone(&pending_plugin_events),
            }));
        let discovered = match discovery_registry.discover().await {
            Ok(discovered) => discovered,
            Err(error) => {
                let pending_events = pending_plugin_events.drain();
                let emitted_discovery_event = !pending_events.is_empty();
                self.append_plugin_events(options, pending_events).await?;
                if !emitted_discovery_event {
                    self.emit_plugin_discovery_error(options, &error).await?;
                }
                return Err(HarnessError::Other(
                    PLUGIN_DISCOVERY_FAILED_MESSAGE.to_owned(),
                ));
            }
        };
        self.append_plugin_events(options, pending_plugin_events.drain())
            .await?;
        for plugin in discovered {
            let plugin_id = plugin.record.manifest.plugin_id();
            if matches!(
                registry.state(&plugin_id),
                Some(harness_plugin::PluginLifecycleState::Activated)
            ) {
                continue;
            }
            if registry.is_plugin_enabled(&plugin_id) == Some(false) {
                continue;
            }
            let from_state = registry
                .state(&plugin_id)
                .map(plugin_state_discriminant)
                .unwrap_or(PluginLifecycleStateDiscriminant::Validated);
            match registry.activate(&plugin_id).await {
                Ok(()) => {
                    self.emit_plugin_loaded(options, &plugin.record, from_state)
                        .await?;
                }
                Err(error) => {
                    if matches!(
                        registry.state(&plugin_id),
                        Some(harness_plugin::PluginLifecycleState::Failed(_))
                    ) {
                        self.emit_plugin_failed(options, &plugin.record).await?;
                    } else {
                        self.emit_plugin_rejected(options, &plugin.record, &error)
                            .await?;
                    }
                    return Err(HarnessError::Other(
                        PLUGIN_ACTIVATION_FAILED_MESSAGE.to_owned(),
                    ));
                }
            }
        }
        Ok(())
    }

    async fn append_plugin_events(
        &self,
        options: &SessionOptions,
        events: Vec<Event>,
    ) -> Result<(), HarnessError> {
        if events.is_empty() {
            return Ok(());
        }
        self.inner
            .event_store
            .append(options.tenant_id, options.session_id, &events)
            .await
            .map_err(HarnessError::Journal)?;
        Ok(())
    }

    async fn emit_plugin_loaded(
        &self,
        options: &SessionOptions,
        record: &ManifestRecord,
        from_state: PluginLifecycleStateDiscriminant,
    ) -> Result<(), HarnessError> {
        let manifest = &record.manifest;
        self.inner
            .event_store
            .append(
                options.tenant_id,
                options.session_id,
                &[Event::PluginLoaded(PluginLoadedEvent {
                    tenant_id: options.tenant_id,
                    plugin_id: manifest.plugin_id(),
                    plugin_name: manifest.name.to_string(),
                    plugin_version: manifest.version.to_string(),
                    trust_level: manifest.trust_level,
                    capabilities: plugin_capabilities_summary(manifest),
                    manifest_origin: manifest_origin_ref(&record.origin),
                    manifest_hash: record.manifest_hash,
                    from_state,
                    at: harness_contracts::now(),
                })],
            )
            .await
            .map_err(HarnessError::Journal)?;
        Ok(())
    }

    async fn emit_plugin_rejected(
        &self,
        options: &SessionOptions,
        record: &ManifestRecord,
        error: &PluginError,
    ) -> Result<(), HarnessError> {
        let manifest = &record.manifest;
        self.inner
            .event_store
            .append(
                options.tenant_id,
                options.session_id,
                &[Event::PluginRejected(PluginRejectedEvent {
                    tenant_id: options.tenant_id,
                    plugin_id: manifest.plugin_id(),
                    plugin_name: manifest.name.to_string(),
                    plugin_version: manifest.version.to_string(),
                    trust_level: manifest.trust_level,
                    manifest_origin: manifest_origin_ref(&record.origin),
                    manifest_hash: record.manifest_hash,
                    reason: rejection_reason(error),
                    at: harness_contracts::now(),
                })],
            )
            .await
            .map_err(HarnessError::Journal)?;
        Ok(())
    }

    async fn emit_plugin_failed(
        &self,
        options: &SessionOptions,
        record: &ManifestRecord,
    ) -> Result<(), HarnessError> {
        let manifest = &record.manifest;
        self.inner
            .event_store
            .append(
                options.tenant_id,
                options.session_id,
                &[Event::PluginFailed(PluginFailedEvent {
                    tenant_id: options.tenant_id,
                    plugin_id: manifest.plugin_id(),
                    plugin_name: manifest.name.to_string(),
                    plugin_version: manifest.version.to_string(),
                    trust_level: manifest.trust_level,
                    manifest_origin: manifest_origin_ref(&record.origin),
                    manifest_hash: record.manifest_hash,
                    failure: PLUGIN_FAILURE_WITHHELD_MESSAGE.to_owned(),
                    at: harness_contracts::now(),
                })],
            )
            .await
            .map_err(HarnessError::Journal)?;
        Ok(())
    }

    async fn emit_plugin_discovery_error(
        &self,
        options: &SessionOptions,
        error: &PluginError,
    ) -> Result<(), HarnessError> {
        if let PluginError::ManifestLoader(ManifestLoaderError::Validation(failure)) = error {
            self.inner
                .event_store
                .append(
                    options.tenant_id,
                    options.session_id,
                    &[Event::ManifestValidationFailed(
                        ManifestValidationFailedEvent {
                            tenant_id: options.tenant_id,
                            manifest_origin: failure
                                .origin
                                .as_ref()
                                .map(manifest_origin_ref)
                                .unwrap_or_else(|| ManifestOriginRef::File {
                                    path: "<unknown>".to_owned(),
                                }),
                            partial_name: failure.partial_name.clone(),
                            partial_version: failure.partial_version.clone(),
                            raw_bytes_hash: failure.raw_bytes_hash,
                            failure: manifest_validation_failure_for_event(&failure.failure),
                            at: harness_contracts::now(),
                        },
                    )],
                )
                .await
                .map_err(HarnessError::Journal)?;
        }
        Ok(())
    }
}

fn plugin_capabilities_summary(
    manifest: &harness_plugin::PluginManifest,
) -> PluginCapabilitiesSummary {
    PluginCapabilitiesSummary {
        tools: manifest
            .capabilities
            .tools
            .len()
            .try_into()
            .unwrap_or(u16::MAX),
        hooks: manifest
            .capabilities
            .hooks
            .len()
            .try_into()
            .unwrap_or(u16::MAX),
        mcp_servers: manifest
            .capabilities
            .mcp_servers
            .len()
            .try_into()
            .unwrap_or(u16::MAX),
        skills: manifest
            .capabilities
            .skills
            .len()
            .try_into()
            .unwrap_or(u16::MAX),
        steering: manifest.capabilities.steering,
        memory_provider: manifest.capabilities.memory_provider.is_some(),
        coordinator: manifest.capabilities.coordinator_strategy.is_some(),
    }
}

fn manifest_origin_ref(origin: &ManifestOrigin) -> ManifestOriginRef {
    match origin {
        ManifestOrigin::File { .. } => ManifestOriginRef::File {
            path: PLUGIN_EVENT_LOCAL_ORIGIN.to_owned(),
        },
        ManifestOrigin::CargoExtension { .. } => ManifestOriginRef::CargoExtension {
            binary: PLUGIN_EVENT_CARGO_EXTENSION_ORIGIN.to_owned(),
        },
        ManifestOrigin::RemoteRegistry { .. } => ManifestOriginRef::RemoteRegistry {
            endpoint: PLUGIN_EVENT_REMOTE_ORIGIN.to_owned(),
        },
        _ => ManifestOriginRef::File {
            path: PLUGIN_EVENT_LOCAL_ORIGIN.to_owned(),
        },
    }
}

fn manifest_validation_failure_for_event(
    failure: &harness_contracts::ManifestValidationFailure,
) -> harness_contracts::ManifestValidationFailure {
    match failure {
        harness_contracts::ManifestValidationFailure::SyntaxError { .. } => {
            harness_contracts::ManifestValidationFailure::SyntaxError {
                details: PLUGIN_EVENT_DETAILS_WITHHELD.to_owned(),
            }
        }
        harness_contracts::ManifestValidationFailure::SchemaViolation { json_pointer, .. } => {
            harness_contracts::ManifestValidationFailure::SchemaViolation {
                json_pointer: json_pointer.clone(),
                details: PLUGIN_EVENT_DETAILS_WITHHELD.to_owned(),
            }
        }
        harness_contracts::ManifestValidationFailure::UnsupportedSchemaVersion {
            found,
            supported,
        } => harness_contracts::ManifestValidationFailure::UnsupportedSchemaVersion {
            found: *found,
            supported: supported.clone(),
        },
        harness_contracts::ManifestValidationFailure::CargoExtensionMetadataMalformed {
            ..
        } => harness_contracts::ManifestValidationFailure::CargoExtensionMetadataMalformed {
            details: PLUGIN_EVENT_DETAILS_WITHHELD.to_owned(),
        },
        harness_contracts::ManifestValidationFailure::RemoteIntegrityMismatch {
            got_etag, ..
        } => harness_contracts::ManifestValidationFailure::RemoteIntegrityMismatch {
            expected_etag: PLUGIN_EVENT_DETAILS_WITHHELD.to_owned(),
            got_etag: got_etag
                .as_ref()
                .map(|_| PLUGIN_EVENT_DETAILS_WITHHELD.to_owned()),
        },
        _ => harness_contracts::ManifestValidationFailure::SyntaxError {
            details: PLUGIN_EVENT_DETAILS_WITHHELD.to_owned(),
        },
    }
}

fn plugin_state_discriminant(
    state: harness_plugin::PluginLifecycleState,
) -> PluginLifecycleStateDiscriminant {
    match state {
        harness_plugin::PluginLifecycleState::Validated => {
            PluginLifecycleStateDiscriminant::Validated
        }
        harness_plugin::PluginLifecycleState::Activating => {
            PluginLifecycleStateDiscriminant::Activating
        }
        harness_plugin::PluginLifecycleState::Activated => {
            PluginLifecycleStateDiscriminant::Activated
        }
        harness_plugin::PluginLifecycleState::Deactivating => {
            PluginLifecycleStateDiscriminant::Deactivating
        }
        harness_plugin::PluginLifecycleState::Deactivated => {
            PluginLifecycleStateDiscriminant::Deactivated
        }
        harness_plugin::PluginLifecycleState::Rejected(_) => {
            PluginLifecycleStateDiscriminant::Rejected
        }
        harness_plugin::PluginLifecycleState::Failed(_) => PluginLifecycleStateDiscriminant::Failed,
        _ => PluginLifecycleStateDiscriminant::Failed,
    }
}

pub(super) struct BufferedPluginEventSink {
    pub(super) pending_session_events: Arc<PendingSessionEvents>,
}

impl PluginEventSink for BufferedPluginEventSink {
    fn emit(&self, event: Event) {
        self.pending_session_events.push(event);
    }
}

fn rejection_reason(error: &PluginError) -> RejectionReason {
    match error {
        PluginError::SignatureInvalid { details } => RejectionReason::SignatureInvalid {
            details: if details.is_empty() {
                String::new()
            } else {
                PLUGIN_EVENT_DETAILS_WITHHELD.to_owned()
            },
        },
        PluginError::UnknownSigner(signer) => RejectionReason::UnknownSigner {
            signer: if signer.is_empty() {
                String::new()
            } else {
                PLUGIN_EVENT_DETAILS_WITHHELD.to_owned()
            },
        },
        PluginError::SignerRevoked { signer, revoked_at } => RejectionReason::SignerRevoked {
            signer: if signer.is_empty() {
                String::new()
            } else {
                PLUGIN_EVENT_DETAILS_WITHHELD.to_owned()
            },
            revoked_at: *revoked_at,
        },
        PluginError::SlotOccupied { slot, occupant } => RejectionReason::SlotOccupied {
            slot: if format!("{slot:?}").is_empty() {
                String::new()
            } else {
                PLUGIN_EVENT_DETAILS_WITHHELD.to_owned()
            },
            occupant: if occupant.0.is_empty() {
                String::new()
            } else {
                PLUGIN_EVENT_DETAILS_WITHHELD.to_owned()
            },
        },
        PluginError::DependencyUnsatisfied {
            dependency,
            requirement,
        } => RejectionReason::DependencyUnsatisfied {
            dependency: if dependency.is_empty() {
                String::new()
            } else {
                PLUGIN_EVENT_DETAILS_WITHHELD.to_owned()
            },
            requirement: if requirement.is_empty() {
                String::new()
            } else {
                PLUGIN_EVENT_DETAILS_WITHHELD.to_owned()
            },
        },
        PluginError::DependencyCycle(cycle) => RejectionReason::DependencyCycle {
            cycle: cycle
                .iter()
                .map(|_| PLUGIN_EVENT_DETAILS_WITHHELD.to_owned())
                .collect(),
        },
        PluginError::AdmissionDenied { policy } => RejectionReason::AdmissionDenied {
            policy: if policy.is_empty() {
                String::new()
            } else {
                PLUGIN_EVENT_DETAILS_WITHHELD.to_owned()
            },
        },
        PluginError::NamespaceConflict { details } => RejectionReason::NamespaceConflict {
            details: if details.is_empty() {
                String::new()
            } else {
                PLUGIN_EVENT_DETAILS_WITHHELD.to_owned()
            },
        },
        PluginError::TrustMismatch {
            declared,
            source_label: _,
        } => RejectionReason::AdmissionDenied {
            policy: format!("trust mismatch: declared {declared:?}, source withheld"),
        },
        PluginError::HarnessVersionIncompatible { required, actual } => {
            RejectionReason::AdmissionDenied {
                policy: format!(
                    "harness version incompatible: required {}, actual {}",
                    safe_nonempty(required),
                    safe_nonempty(actual)
                ),
            }
        }
        PluginError::ActiveDependents(dependents) => RejectionReason::AdmissionDenied {
            policy: format!("active dependents: {}", dependents.len()),
        },
        PluginError::InvalidManifest(details) => RejectionReason::NamespaceConflict {
            details: if details.is_empty() {
                String::new()
            } else {
                PLUGIN_EVENT_DETAILS_WITHHELD.to_owned()
            },
        },
        PluginError::Registration(error) => RejectionReason::AdmissionDenied {
            policy: if error.to_string().is_empty() {
                String::new()
            } else {
                PLUGIN_EVENT_DETAILS_WITHHELD.to_owned()
            },
        },
        PluginError::ActivateFailed(details)
        | PluginError::DeactivateFailed(details)
        | PluginError::Builder(details) => RejectionReason::AdmissionDenied {
            policy: if details.is_empty() {
                String::new()
            } else {
                PLUGIN_EVENT_DETAILS_WITHHELD.to_owned()
            },
        },
        PluginError::SignerStore(error) => RejectionReason::AdmissionDenied {
            policy: if error.to_string().is_empty() {
                String::new()
            } else {
                PLUGIN_EVENT_DETAILS_WITHHELD.to_owned()
            },
        },
        PluginError::ManifestLoader(ManifestLoaderError::Io(error))
        | PluginError::RuntimeLoader(harness_plugin::RuntimeLoaderError::LoadFailed(error))
        | PluginError::RuntimeLoader(harness_plugin::RuntimeLoaderError::UnsupportedOrigin(
            error,
        )) => RejectionReason::AdmissionDenied {
            policy: if error.is_empty() {
                String::new()
            } else {
                PLUGIN_EVENT_DETAILS_WITHHELD.to_owned()
            },
        },
        PluginError::ManifestLoader(ManifestLoaderError::UnsupportedSource(source)) => {
            RejectionReason::AdmissionDenied {
                policy: if source.is_empty() {
                    String::new()
                } else {
                    PLUGIN_EVENT_DETAILS_WITHHELD.to_owned()
                },
            }
        }
        PluginError::ManifestLoader(ManifestLoaderError::Validation(failure)) => {
            RejectionReason::AdmissionDenied {
                policy: if failure.details.is_empty() {
                    String::new()
                } else {
                    PLUGIN_EVENT_DETAILS_WITHHELD.to_owned()
                },
            }
        }
        PluginError::RuntimeLoader(harness_plugin::RuntimeLoaderError::PluginNotFound(name)) => {
            RejectionReason::DependencyUnsatisfied {
                dependency: if name.to_string().is_empty() {
                    String::new()
                } else {
                    PLUGIN_EVENT_DETAILS_WITHHELD.to_owned()
                },
                requirement: PLUGIN_EVENT_DETAILS_WITHHELD.to_owned(),
            }
        }
    }
}

fn safe_nonempty(value: &str) -> String {
    if value.is_empty() {
        String::new()
    } else {
        PLUGIN_EVENT_DETAILS_WITHHELD.to_owned()
    }
}
