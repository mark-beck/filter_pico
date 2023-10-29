use defmt::Format;

#[derive(Format)]
pub struct Context {
    pub state: State,
    pub config: Config,
    pub network_state: NetworkState,
    pub clock_skew: u64,
}

#[derive(Format, PartialEq, Eq, Clone, Copy)]
pub enum NetworkState {
    Disconnected,
    Registered,
}

#[derive(Format)]
pub struct State {
    pub filter_state: FilterState,
    pub last_state_change: u64,
    pub waterlevel: Option<u64>,
    pub measurement_error: Option<u64>,
    pub leak: Option<u64>,
}

#[derive(Format)]
pub struct Config {
    pub waterlevel_fill_start: u64,
    pub waterlevel_fill_end: u64,
    pub clean_before_fill_duration: u64,
    pub clean_after_fill_duration: u64,
    pub leak_protection: bool,
}

#[derive(Format, PartialEq, Eq)]
pub enum FilterState {
    CleanBeforeFill,
    CleanAfterFill,
    Fill,
    Idle,
    ForcedFill(u64),
    ForcedClean(u64),
    ForcedIdle(u64),
}
