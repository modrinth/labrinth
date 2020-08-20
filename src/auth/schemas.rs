use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// CredentialsType represents several different credential types, like password credentials,
/// passwordless credentials,
#[derive(Serialize, Deserialize)]
pub struct CredentialsType(pub String);

#[derive(Serialize, Deserialize)]
pub struct Id(pub i32);

#[derive(Serialize, Deserialize)]
pub struct Uuid(pub String);

#[derive(Serialize, Deserialize)]
pub struct Identity {
    pub id: Uuid,
    /// RecoveryAddresses contains all the addresses that can be used to recover an identity.
    pub recovery_addresses: Option<Vec<RecoveryAddress>>,
    /// SchemaID is the ID of the JSON Schema to be used for validating the identity's traits.
    pub schema_id: String,
    /// SchemaURL is the URL of the endpoint where the identity's traits schema can be fetched from.
    /// format: url
    pub schema_url: Option<String>,
    pub traits: Traits,
    /// VerifiableAddresses contains all the addresses that can be verified by the user.
    pub verifiable_addresses: Option<Vec<VerifiableAddress>>,
}

#[derive(Serialize, Deserialize)]
pub struct Message {
    pub context: Option<Value>,
    pub id: Option<Id>,
    pub text: Option<String>,
    #[serde(rename = "type")]
    pub typez: Type,
}

#[derive(Serialize, Deserialize)]
pub struct ProviderCredentialsConfig {
    pub provider: Option<String>,
    pub subject: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct RecoveryAddress {
    pub id: Uuid,
    pub value: String,
    pub via: RecoveryAddressType,
}

#[derive(Serialize, Deserialize)]
pub struct RecoveryAddressType(pub String);

#[derive(Serialize, Deserialize)]
pub struct RequestMethodConfig {
    /// Action should be used as the form action URL <form action="{{ .Action }}" method="post">.
    pub action: String,
    /// Fields contains multiple fields
    pub fields: Vec<FormField>,
    pub messages: Option<Vec<Message>>,
    /// Method is the form method (e.g. POST)
    pub method: String,
}

#[derive(Serialize, Deserialize)]
pub struct State(pub String);

#[derive(Serialize, Deserialize)]
pub struct Traits {}

#[derive(Serialize, Deserialize)]
pub struct Type(pub String);

#[derive(Serialize, Deserialize)]
pub struct VerifiableAddress {
    pub expires_at: DateTime<Utc>,
    pub id: Uuid,
    pub value: String,
    pub verified: bool,
    pub verified_at: Option<DateTime<Utc>>,
    pub via: VerifiableAddressType,
}

#[derive(Serialize, Deserialize)]
pub struct VerifiableAddressType(pub String);

#[derive(Serialize, Deserialize)]
pub struct CompleteSelfServiceBrowserSettingsStrategyProfileFlowPayload {
    /// RequestID is request ID. in: query
    pub request_id: Option<String>,
    /// Traits contains all of the identity's traits. type: string format: binary
    pub traits: Traits,
}

#[derive(Serialize, Deserialize)]
pub struct ErrorContainer {
    pub errors: Option<Value>,
    pub id: Option<Uuid>,
}

/// HTMLForm represents a HTML Form. The container can work with both HTTP Form and JSON requests
#[derive(Serialize, Deserialize)]
pub struct Form {
    /// Action should be used as the form action URL <form action="{{ .Action }}" method="post">
    pub action: String,
    /// Fields contains multiple fields
    pub fields: Vec<FormField>,
    pub messages: Option<Vec<Message>>,
    /// Method is the form method (e.g. POST)
    pub method: String,
}

/// Field represents a HTML Form Field
#[derive(Serialize, Deserialize)]
pub struct FormField {
    /// Disabled is the equivalent of <input {{if .Disabled}}disabled{{end}}">
    pub disabled: Option<bool>,
    pub messages: Option<Vec<Message>>,
    /// Name is the equivalent of <input name="{{.Name}}">
    pub name: String,
    /// Pattern is the equivalent of <input pattern="{{.Pattern}}">
    pub pattern: Option<String>,
    /// Required is the equivalent of <input required="{{.Required}}">
    pub required: Option<bool>,
    #[serde(rename = "type")]
    /// Type is the equivalent of <input type="{{.Type}}">
    pub typez: String,
    /// Value is the equivalent of <input value="{{.Value}}">
    pub value: Value,
}

/// Error response
#[derive(Serialize, Deserialize)]
pub struct GenericError {
    pub error: Option<GenericErrorPayload>,
}

#[derive(Serialize, Deserialize)]
pub struct GenericErrorPayload {
    /// Code represents the error status code (404, 403, 401, ...).
    pub code: Option<i64>,
    /// Debug contains debug information. This is usually not available and has to be enabled.
    pub debug: Option<String>,
    pub details: Option<Value>,
    pub message: Option<String>,
    pub reason: Option<String>,
    pub request: Option<String>,
    pub status: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct HealthNotReadyStatus {
    /// Errors contains a list of errors that caused the not ready status.
    pub errors: Option<Vec<Value>>,
}

#[derive(Serialize, Deserialize)]
pub struct HealthStatus {
    /// Status always contains "ok".
    pub status: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct LoginRequest {
    pub active: Option<CredentialsType>,
    pub expires_at: DateTime<Utc>,
    /// Forced stores whether this login request should enforce reauthentication.
    pub forced: Option<bool>,
    pub id: Uuid,
    /// IssuedAt is the time (UTC) when the request occurred.
    pub issued_at: DateTime<Utc>,
    pub messages: Option<Vec<Message>>,
    /// Methods contains context for all enabled login methods. If a login request has been
    /// processed, but for example the password is incorrect, this will contain error messages.
    pub methods: Vec<LoginRequestMethod>,
    /// RequestURL is the initial URL that was requested from ORY Kratos. It can be used to forward
    /// information contained in the URL's path or query for example.
    pub request_url: String,
}

#[derive(Serialize, Deserialize)]
pub struct LoginRequestMethod {
    pub config: LoginRequestMethodConfig,
    pub method: CredentialsType,
}

#[derive(Serialize, Deserialize)]
pub struct LoginRequestMethodConfig {
    /// Action should be used as the form action URL <form action="{{ .Action }}" method="post">.
    pub action: String,
    /// Fields contains multiple fields
    pub fields: Vec<FormField>,
    pub messages: Option<Vec<Message>>,
    /// Method is the form method (e.g. POST)
    pub method: String,
    /// Providers is set for the "oidc" request method.
    pub providers: Option<FormField>,
}

/// Request presents a recovery request
#[derive(Serialize, Deserialize)]
pub struct RecoveryRequest {
    /// Active, if set, contains the registration method that is being used.
    /// It is initially not set.
    pub active: Option<String>,
    /// ExpiresAt is the time (UTC) when the request expires. If the user still wishes to update the
    /// setting, a new request has to be initiated.
    pub expires_at: DateTime<Utc>,
    pub id: Uuid,
    /// IssuedAt is the time (UTC) when the request occurred.
    pub issued_at: DateTime<Utc>,
    pub messages: Option<Vec<Message>>,
    /// Methods contains context for all account recovery methods. If a registration request has
    /// been processed, but for example the password is incorrect, this will contain error messages.
    pub methods: Vec<RecoveryRequestMethod>,
    /// RequestURL is the initial URL that was requested from ORY Kratos. It can be used to forward
    /// information contained in the URL's path or query for example.
    pub request_url: Option<String>,
    pub state: State,
}

#[derive(Serialize, Deserialize)]
pub struct RecoveryRequestMethod {
    pub config: Option<RequestMethodConfig>,
    /// Method contains the request credentials type.
    pub method: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct RegistrationRequest {
    pub active: Option<CredentialsType>,
    /// ExpiresAt is the time (UTC) when the request expires. If the user still wishes to log in, a
    /// new request has to be initiated.
    pub expires_at: DateTime<Utc>,
    pub id: Uuid,
    /// IssuedAt is the time (UTC) when the request occurred.
    pub issued_at: DateTime<Utc>,
    pub messages: Option<Vec<Message>>,
    /// Methods contains context for all enabled registration methods. If a registration request has
    /// been processed, but for example the password is incorrect, this will contain error messages
    pub methods: Vec<RegistrationRequestMethod>,
    /// RequestURL is the initial URL that was requested from ORY Kratos. It can be used to forward
    /// information contained in the URL's path or query for example.
    pub request_url: String,
}

#[derive(Serialize, Deserialize)]
pub struct RegistrationRequestMethod {
    pub config: Option<RegistrationRequestMethodConfig>,
    pub method: Option<CredentialsType>,
}

#[derive(Serialize, Deserialize)]
pub struct RegistrationRequestMethodConfig {
    /// Action should be used as the form action URL <form action="{{ .Action }}" method="post">.
    pub action: String,
    /// Fields contains multiple fields
    pub fields: Vec<FormField>,
    pub messages: Option<Vec<Message>>,
    /// Method is the form method (e.g. POST)
    pub method: String,
    /// Providers is set for the "oidc" request method.
    pub providers: Option<FormField>,
}

#[derive(Serialize, Deserialize)]
pub struct Session {
    pub authenticated_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub identity: Identity,
    pub issued_at: DateTime<Utc>,
    pub sid: Uuid,
}

/// Request presents a settings request
#[derive(Serialize, Deserialize)]
pub struct SettingsRequest {
    /// Active, if set, contains the registration method that is being used.
    /// It is initially not set.
    pub active: Option<String>,
    /// ExpiresAt is the time (UTC) when the request expires. If the user still wishes to update the
    /// setting, a new request has to be initiated.
    pub expires_at: DateTime<Utc>,
    pub id: Uuid,
    pub identity: Identity,
    /// IssuedAt is the time (UTC) when the request occurred.
    pub issued_at: DateTime<Utc>,
    pub messages: Option<Vec<Message>>,
    /// Methods contains context for all enabled registration methods. If a registration request has
    /// been processed, but for example the password is incorrect, this will contain error messages.
    pub methods: Vec<SettingsRequestMethod>,
    /// RequestURL is the initial URL that was requested from ORY Kratos. It can be used to forward
    /// information contained in the URL's path or query for example.
    pub request_url: String,
    pub state: State,
}

#[derive(Serialize, Deserialize)]
pub struct SettingsRequestMethod {
    pub config: RequestMethodConfig,
    /// Method contains the request credentials type.
    pub method: Option<String>,
}

/// Request presents a verification request
#[derive(Serialize, Deserialize)]
pub struct VerificationRequest {
    /// ExpiresAt is the time (UTC) when the request expires. If the user still wishes to verify the
    /// address, a new request has to be initiated.
    pub expires_at: Option<DateTime<Utc>>,
    ///HTMLForm represents a HTML Form. The container can work with both HTTP Form and JSON requests
    pub form: Option<Form>,
    #[derive(Serialize, Deserialize)]
    pub id: Option<Uuid>,
    /// IssuedAt is the time (UTC) when the request occurred.
    pub issued_at: Option<DateTime<Utc>>,
    pub messages: Option<Vec<Message>>,
    /// RequestURL is the initial URL that was requested from ORY Kratos. It can be used to forward
    /// information contained in the URL's path or query for example.
    pub request_url: Option<String>,
    /// Success, if true, implies that the request was completed successfully.
    pub success: Option<bool>,
    pub via: Option<VerifiableAddressType>,
}

#[derive(Serialize, Deserialize)]
pub struct Version {
    /// Version is the service's version.
    pub version: String,
}
