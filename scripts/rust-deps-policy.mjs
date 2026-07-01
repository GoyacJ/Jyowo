export const upstreamHeldRustDependencies = [
  {
    name: 'generic-array',
    current: '0.14.7',
    available: '0.14.9',
    owner: 'crypto-common 0.1.7',
    constraint: 'exact dependency required by the RustCrypto `digest 0.10` chain used by Tauri/Wry SHA-2 code',
  },
  {
    name: 'matchit',
    current: '0.8.4',
    available: '0.8.6',
    owner: 'axum 0.8.9',
    constraint: 'exact dependency selected by the latest stable Axum release',
  },
  {
    name: 'toml',
    current: '0.8.2',
    available: '0.8.23',
    owner: 'system-deps 6.2.2',
    constraint: 'Linux GTK/Tauri build dependency chain',
  },
  {
    name: 'toml_datetime',
    current: '0.6.3',
    available: '0.6.11',
    owner: 'proc-macro-crate 2.0.2',
    constraint: 'exact dependency required by GTK proc-macro chain',
  },
  {
    name: 'toml_edit',
    current: '0.20.2',
    available: '0.20.7',
    owner: 'proc-macro-crate 2.0.2',
    constraint: 'exact dependency required by GTK proc-macro chain',
  },
]
