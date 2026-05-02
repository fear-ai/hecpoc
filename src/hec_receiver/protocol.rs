use super::config::parse_optional_u64;

const DEFAULT_SUCCESS: u16 = 0;
const DEFAULT_TOKEN_REQUIRED: u16 = 2;
const DEFAULT_INVALID_AUTHORIZATION: u16 = 3;
const DEFAULT_INVALID_TOKEN: u16 = 4;
const DEFAULT_NO_DATA: u16 = 5;
const DEFAULT_INVALID_DATA_FORMAT: u16 = 6;
const DEFAULT_SERVER_BUSY: u16 = 9;
const DEFAULT_EVENT_FIELD_REQUIRED: u16 = 12;
const DEFAULT_EVENT_FIELD_BLANK: u16 = 13;
const DEFAULT_HANDLING_INDEXED_FIELDS: u16 = 15;
const DEFAULT_HEALTH: u16 = 17;

#[derive(Debug, Clone)]
pub struct Protocol {
    pub success: u16,
    pub token_required: u16,
    pub invalid_authorization: u16,
    pub invalid_token: u16,
    pub no_data: u16,
    pub invalid_data_format: u16,
    pub server_busy: u16,
    pub event_field_required: u16,
    pub event_field_blank: u16,
    pub handling_indexed_fields: u16,
    pub health: u16,
}

impl Default for Protocol {
    fn default() -> Self {
        Self {
            success: DEFAULT_SUCCESS,
            token_required: DEFAULT_TOKEN_REQUIRED,
            invalid_authorization: DEFAULT_INVALID_AUTHORIZATION,
            invalid_token: DEFAULT_INVALID_TOKEN,
            no_data: DEFAULT_NO_DATA,
            invalid_data_format: DEFAULT_INVALID_DATA_FORMAT,
            server_busy: DEFAULT_SERVER_BUSY,
            event_field_required: DEFAULT_EVENT_FIELD_REQUIRED,
            event_field_blank: DEFAULT_EVENT_FIELD_BLANK,
            handling_indexed_fields: DEFAULT_HANDLING_INDEXED_FIELDS,
            health: DEFAULT_HEALTH,
        }
    }
}

impl Protocol {
    pub fn apply_env(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.success = parse_code("HEC_SUCCESS", self.success)?;
        self.token_required = parse_code("HEC_TOKEN_REQUIRED", self.token_required)?;
        self.invalid_authorization =
            parse_code("HEC_INVALID_AUTHORIZATION", self.invalid_authorization)?;
        self.invalid_token = parse_code("HEC_INVALID_TOKEN", self.invalid_token)?;
        self.no_data = parse_code("HEC_NO_DATA", self.no_data)?;
        self.invalid_data_format = parse_code("HEC_INVALID_DATA_FORMAT", self.invalid_data_format)?;
        self.server_busy = parse_code("HEC_SERVER_BUSY", self.server_busy)?;
        self.event_field_required =
            parse_code("HEC_EVENT_FIELD_REQUIRED", self.event_field_required)?;
        self.event_field_blank = parse_code("HEC_EVENT_FIELD_BLANK", self.event_field_blank)?;
        self.handling_indexed_fields =
            parse_code("HEC_HANDLING_INDEXED_FIELDS", self.handling_indexed_fields)?;
        self.health = parse_code("HEC_HEALTH", self.health)?;
        Ok(())
    }
}

fn parse_code(name: &str, default: u16) -> Result<u16, Box<dyn std::error::Error>> {
    Ok(match parse_optional_u64(name)? {
        Some(value) => value.try_into()?,
        None => default,
    })
}
