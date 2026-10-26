/// An interpretation of a clue.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ClueInterp {
    Mistake,
    Reactive,
    Play,
    Save,
    PlayLooksSave,
    Discard,
    Lock,
    Reveal,
    Fix,
    Stall,
    Distribution,
    Useless,
}

/// An interpretation of a play.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum PlayInterp {
    None,
    Mistake,
    OrderCM,
}

/// An interpretation of a discard.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum DiscardInterp {
    None,
    Mistake,
    Sarcastic,
    GentlemansDiscard,
    Emergency,
    Positional,
    Sacrifice,
}

/// An interpretation of a game action.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Interp {
    Clue(ClueInterp),
    Play(PlayInterp),
    Discard(DiscardInterp),
}

impl From<ClueInterp> for Interp {
    fn from(interp: ClueInterp) -> Self {
        Self::Clue(interp)
    }
}

impl From<PlayInterp> for Interp {
    fn from(interp: PlayInterp) -> Self {
        Self::Play(interp)
    }
}

impl From<DiscardInterp> for Interp {
    fn from(interp: DiscardInterp) -> Self {
        Self::Discard(interp)
    }
}
