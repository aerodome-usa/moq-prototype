use super::StateMachine;

#[derive(Debug)]
pub struct EchoMachine {
    latest_position: Option<Position>,
    pending: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Position {
    pub drone_id: String,
    pub latitude: f64,
    pub longitude: f64,
    pub altitude_m: f64,
    pub heading_deg: f64,
    pub speed_mps: f64,
    pub timestamp: u64,
}

impl EchoMachine {
    pub fn new() -> Self {
        Self {
            latest_position: None,
            pending: false,
        }
    }

    fn update_position(&mut self, pos: Position) {
        self.latest_position = Some(pos);
        self.pending = true;
    }

    fn poll_position(&mut self) -> Option<Position> {
        if self.pending {
            self.pending = false;
            self.latest_position.clone()
        } else {
            None
        }
    }
}

impl Default for EchoMachine {
    fn default() -> Self {
        Self::new()
    }
}

pub enum EchoInput {
    Position(Position),
}

pub enum EchoOutput {
    Position(Position),
}

impl StateMachine for EchoMachine {
    type Input = EchoInput;
    type Output = EchoOutput;

    fn process_input(&mut self, input: Self::Input) {
        match input {
            EchoInput::Position(pos) => self.update_position(pos),
        }
    }

    fn poll_output(&mut self) -> Option<Self::Output> {
        self.poll_position().map(EchoOutput::Position)
    }
}
