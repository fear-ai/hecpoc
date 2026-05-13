const DEFAULT_SUCCESS: u16 = 0;
const DEFAULT_TOKEN_DISABLED: u16 = 1;
const DEFAULT_TOKEN_REQUIRED: u16 = 2;
const DEFAULT_INVALID_AUTHORIZATION: u16 = 3;
const DEFAULT_INVALID_TOKEN: u16 = 4;
const DEFAULT_NO_DATA: u16 = 5;
const DEFAULT_INVALID_DATA_FORMAT: u16 = 6;
const DEFAULT_INCORRECT_INDEX: u16 = 7;
const DEFAULT_SERVER_BUSY: u16 = 9;
const DEFAULT_EVENT_FIELD_REQUIRED: u16 = 12;
const DEFAULT_EVENT_FIELD_BLANK: u16 = 13;
const DEFAULT_ACK_DISABLED: u16 = 14;
const DEFAULT_HANDLING_INDEXED_FIELDS: u16 = 15;
const DEFAULT_QUERY_STRING_AUTHORIZATION_DISABLED: u16 = 16;
const DEFAULT_HEALTH_OK: u16 = 17;
const DEFAULT_HEALTH_UNHEALTHY: u16 = 18;
const DEFAULT_SERVER_SHUTTING_DOWN: u16 = 23;

#[derive(Debug, Clone)]
pub struct Protocol {
    pub success: u16,
    pub token_disabled: u16,
    pub token_required: u16,
    pub invalid_authorization: u16,
    pub invalid_token: u16,
    pub no_data: u16,
    pub invalid_data_format: u16,
    pub incorrect_index: u16,
    pub server_busy: u16,
    pub event_field_required: u16,
    pub event_field_blank: u16,
    pub ack_disabled: u16,
    pub handling_indexed_fields: u16,
    pub query_string_authorization_disabled: u16,
    pub health_ok: u16,
    pub health_unhealthy: u16,
    pub server_shutting_down: u16,
}

impl Default for Protocol {
    fn default() -> Self {
        Self {
            success: DEFAULT_SUCCESS,
            token_disabled: DEFAULT_TOKEN_DISABLED,
            token_required: DEFAULT_TOKEN_REQUIRED,
            invalid_authorization: DEFAULT_INVALID_AUTHORIZATION,
            invalid_token: DEFAULT_INVALID_TOKEN,
            no_data: DEFAULT_NO_DATA,
            invalid_data_format: DEFAULT_INVALID_DATA_FORMAT,
            incorrect_index: DEFAULT_INCORRECT_INDEX,
            server_busy: DEFAULT_SERVER_BUSY,
            event_field_required: DEFAULT_EVENT_FIELD_REQUIRED,
            event_field_blank: DEFAULT_EVENT_FIELD_BLANK,
            ack_disabled: DEFAULT_ACK_DISABLED,
            handling_indexed_fields: DEFAULT_HANDLING_INDEXED_FIELDS,
            query_string_authorization_disabled: DEFAULT_QUERY_STRING_AUTHORIZATION_DISABLED,
            health_ok: DEFAULT_HEALTH_OK,
            health_unhealthy: DEFAULT_HEALTH_UNHEALTHY,
            server_shutting_down: DEFAULT_SERVER_SHUTTING_DOWN,
        }
    }
}
