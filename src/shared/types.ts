export type CliId = "claude-code" | "codex" | "opencode";
export type CliProtocol = "openai-chat" | "openai-responses" | "anthropic-messages";
export type OAuthKind = "anthropic" | "codex";
export type ConnectionAuthType = "api-key" | "bearer";
export type VerificationStatus =
  | "never-tested"
  | "valid"
  | "invalid"
  | "not-online-verified"
  | "user-modified-unverified";

export interface VerificationInfo {
  status: VerificationStatus;
  verifiedAt?: string | null;
  error?: string | null;
}

export interface ProviderConnection {
  id: string;
  templateEndpointId?: string | null;
  credentialSlotId: string;
  protocol: CliProtocol;
  endpoint: string;
  authType: ConnectionAuthType;
  apiKey: string;
  defaultModel: string;
  verification: VerificationInfo;
}

export type PublicProviderConnection = Omit<ProviderConnection, "apiKey">;

export interface PublicProvider {
  id: string;
  name: string;
  kind: "api" | "oauth";
  templateId?: string | null;
  templateName?: string | null;
  templateMode?: "api" | "auth" | null;
  templateCategory?: string | null;
  oauthKind?: OAuthKind | null;
  oauthAccountLabel?: string | null;
  connections: PublicProviderConnection[];
  verificationStatus?: VerificationStatus | null;
  referencedBy: string[];
  revision: number;
  updatedAt: string;
}

interface ProviderBase {
  id: string;
  name: string;
  templateId?: string | null;
  revision: number;
  createdAt: string;
  updatedAt: string;
}

export interface ApiProviderDetail extends ProviderBase {
  profileType: "api";
  connections: ProviderConnection[];
}

export interface OAuthProviderDetail extends ProviderBase {
  profileType: "oauth";
  oauthKind: OAuthKind;
  accountId?: string | null;
  accountLabel?: string | null;
  rawContent: string;
  digest: string;
  manuallyModified: boolean;
  verification: VerificationInfo;
}

export type ProviderDetail = ApiProviderDetail | OAuthProviderDetail;

export interface ApiTarget {
  targetType: "api";
  cliId: CliId;
  providerId: string;
  connectionId: string;
  model: string;
}

export interface OAuthTarget {
  targetType: "oauth";
  cliId: CliId;
  providerId: string;
  model: string;
}

export type ConfigurationTarget = ApiTarget | OAuthTarget;

export interface SavedConfiguration {
  id: string;
  name: string;
  creationOrder: number;
  revision: number;
  targets: ConfigurationTarget[];
  lastAppliedAt?: string | null;
  lastApplySummary?: string | null;
  createdAt: string;
  updatedAt: string;
}

export type ScanStatus =
  | "not-installed"
  | "installed"
  | "detected"
  | "partially-detected"
  | "unmanaged"
  | "externally-overridden"
  | "unreadable"
  | "invalid-config";

export interface SourceFileSnapshot {
  sourceId: string;
  displayPath: string;
  digest?: string | null;
}

export interface CurrentCliConfiguration {
  providerName?: string | null;
  protocol?: CliProtocol | null;
  authKind?: string | null;
  model?: string | null;
  managedProviderId?: string | null;
  sources: SourceFileSnapshot[];
  externallyOverridden: boolean;
  diagnostics: string[];
}

export interface DetectedProviderCandidate {
  id: string;
  sourceProviderId: string;
  suggestedName: string;
  templateId?: string | null;
  protocol?: CliProtocol | null;
  endpoint?: string | null;
  authType?: ConnectionAuthType | null;
  availableModels: string[];
  defaultModel?: string | null;
}

export interface DetectedCli {
  cliId: CliId;
  label: string;
  status: ScanStatus;
  executablePath?: string | null;
  configDirectory: string;
  version?: string | null;
  source: string;
  current?: CurrentCliConfiguration | null;
  providerCandidates?: DetectedProviderCandidate[];
}

export interface ScanSnapshot {
  id: string;
  generatedAt: string;
  items: DetectedCli[];
}

export interface ManualCliLocation {
  cliId: CliId;
  executablePath?: string | null;
  configDirectory?: string | null;
}

export interface AppSettings {
  language: "zh-cn" | "en";
  theme: "light" | "dark" | "system";
  scanOnStartup: boolean;
  plaintextRiskAccepted: boolean;
  revision: number;
  manualLocations: ManualCliLocation[];
}

export type ApplyItemState =
  | "waiting"
  | "writing"
  | "success"
  | "success-unverified"
  | "unchanged"
  | "not-installed"
  | "incompatible"
  | "conflict"
  | "running-blocked"
  | "failed"
  | "cancelled";

export interface FieldChange {
  field: string;
  before?: string | null;
  after?: string | null;
}

export interface ApplyPreviewFile {
  path: string;
  existed: boolean;
  sourceContent?: string | null;
  targetContent: string;
}

export interface ApplyPreviewItem {
  cliId: CliId;
  state: ApplyItemState;
  path?: string | null;
  providerName: string;
  protocol?: CliProtocol | null;
  model: string;
  changes: FieldChange[];
  files: ApplyPreviewFile[];
  warning?: string | null;
}

export interface ApplyPreview {
  id: string;
  configurationId: string;
  configurationRevision: number;
  createdAt: string;
  expiresAt: string;
  items: ApplyPreviewItem[];
}

export interface ApplyRunItem {
  cliId: CliId;
  state: ApplyItemState;
  message?: string | null;
}

export interface ApplyRunSnapshot {
  id: string;
  previewId: string;
  configurationId: string;
  startedAt: string;
  finishedAt?: string | null;
  cancelRequested: boolean;
  items: ApplyRunItem[];
}

export interface BackupMetadata {
  id: string;
  cliId: CliId;
  sourceFileId: string;
  originalPath: string;
  createdAt: string;
  configurationId?: string | null;
  originalDigest?: string | null;
  permissions?: number | null;
  originallyExisted: boolean;
  containsCredentials: boolean;
  relativeBackupPath?: string | null;
}

export interface RestorePreview {
  id: string;
  backupId: string;
  cliId: CliId;
  targetPath: string;
  currentDigest?: string | null;
  restoresTombstone: boolean;
  containsCredentials: boolean;
  expiresAt: string;
}

export interface OAuthSessionSnapshot {
  id: string;
  kind: OAuthKind;
  stage:
    | "starting"
    | "waiting-for-browser"
    | "waiting-for-confirmation"
    | "success"
    | "failed"
    | "cancelled";
  message: string;
  providerId?: string | null;
  startedAt: string;
  finishedAt?: string | null;
}

export interface AppSnapshot {
  catalog: ProviderCatalog;
  settings: AppSettings;
  providers: PublicProvider[];
  configurations: SavedConfiguration[];
  current?: ScanSnapshot | null;
  latestApply?: ApplyRunSnapshot | null;
  configurationStatuses: Record<
    string,
    "applied" | "partially-applied" | "not-applied" | "unable-to-verify" | "no-applicable-cli"
  >;
  appDataDirectory: string;
  backupBytes: number;
  appVersion: string;
}

export interface StartupStatus {
  ready: boolean;
  code?: string | null;
  message?: string | null;
  appDataDirectory: string;
}

export interface CloseState {
  frontendDirty: boolean;
  oauthActive: boolean;
  applyActive: boolean;
}

export interface ApiProviderDraft {
  name: string;
  templateId?: string;
  connections: Array<{
    id?: string;
    templateEndpointId?: string;
    credentialSlotId: string;
    protocol: CliProtocol;
    endpoint: string;
    authType: ConnectionAuthType;
    apiKey: string;
    defaultModel: string;
  }>;
}

export const CLI_IDS: CliId[] = ["claude-code", "codex", "opencode"];

export interface CatalogAuthMode {
  id: string;
  oauthKind: OAuthKind;
}

export interface CliProtocolAdapter {
  protocol: CliProtocol;
  providerPackage: string;
}

export interface CatalogCli {
  id: CliId;
  name: string;
  protocols: CliProtocol[];
  authModes: CatalogAuthMode[];
  protocolAdapters: CliProtocolAdapter[];
}

export interface EndpointAuthOption {
  id: string;
  authType: ConnectionAuthType;
}

export interface ProviderModelTemplate {
  id: string;
  name: string;
  default: boolean;
  context?: number | null;
  output?: number | null;
}

export interface ProviderEndpointTemplate {
  id: string;
  name: string;
  protocol: CliProtocol;
  baseUrl: string;
  credentialSlotId: string;
  authOptions: EndpointAuthOption[];
  defaultAuthOptionId: string;
  models: ProviderModelTemplate[];
}

export interface ApiProviderTemplate {
  mode: "api";
  id: string;
  name: string;
  category: string;
  modelRouting?: boolean;
  credentialSlots: Array<{ id: string; name: string }>;
  endpoints: ProviderEndpointTemplate[];
  unsupportedModels?: UnsupportedProviderModelTemplate[];
}

export interface UnsupportedProviderModelTemplate {
  id: string;
  name: string;
  providerPackage: string;
}

export interface AuthProviderTemplate {
  mode: "auth";
  id: string;
  name: string;
  authKind: OAuthKind;
}

export type ProviderTemplate = ApiProviderTemplate | AuthProviderTemplate;

export interface ApiCliProviderRelation {
  mode: "api";
  id: string;
  cliId: CliId;
  providerTemplateId: string;
  endpointId: string;
  authOptionId: string;
  baseUrl?: string | null;
  providerPackage?: string | null;
  default: boolean;
  nativeProviderIds: string[];
}

export interface AuthCliProviderRelation {
  mode: "auth";
  id: string;
  cliId: CliId;
  providerTemplateId: string;
  authModeId: string;
}

export type CliProviderRelation = ApiCliProviderRelation | AuthCliProviderRelation;

export interface ProviderCatalog {
  schemaVersion: number;
  clis: CatalogCli[];
  providerTemplates: ProviderTemplate[];
  relations: CliProviderRelation[];
  /** Backend-resolved compatibility details, including disabled reasons. */
  providerInfo?: CatalogProviderInfo[];
}

export interface CatalogModelInfo {
  id: string;
  name: string;
  status?: string | null;
  selectable: boolean;
  disabledReason?: string | null;
  context?: number | null;
  output?: number | null;
}

export interface CatalogProviderInfo {
  id: string;
  name: string;
  npm: string;
  env: string[];
  api?: string | null;
  doc: string;
  protocol?: CliProtocol | null;
  authType?: ConnectionAuthType | null;
  endpoint?: string | null;
  selectable: boolean;
  disabledReason?: string | null;
  supportedClis: CliId[];
  models: CatalogModelInfo[];
}

export type CatalogSource = "bundled" | "local";

export interface CatalogStatus {
  source: CatalogSource;
  cachePath: string;
  metadataPath: string;
  fetchedAt?: string | null;
  etag?: string | null;
  digest: string;
  providerCount: number;
  modelCount: number;
  lastError?: string | null;
  updateAvailable: boolean;
}
