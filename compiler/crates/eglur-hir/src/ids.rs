macro_rules! file_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(u32);

        impl $name {
            pub(crate) fn from_index(index: usize) -> Self {
                Self(u32::try_from(index).expect("HIR arena index must fit u32"))
            }

            #[must_use]
            pub const fn get(self) -> u32 {
                self.0
            }

            pub(crate) const fn index(self) -> usize {
                self.0 as usize
            }
        }
    };
}

file_id!(ItemId);
file_id!(BodyId);
file_id!(TypeRefId);
file_id!(OriginId);
file_id!(SyntaxCauseId);

macro_rules! body_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name {
            body: BodyId,
            local: u32,
        }

        impl $name {
            pub(crate) fn from_index(body: BodyId, index: usize) -> Self {
                Self {
                    body,
                    local: u32::try_from(index).expect("body-local HIR index must fit u32"),
                }
            }

            #[must_use]
            pub const fn body(self) -> BodyId {
                self.body
            }

            #[must_use]
            pub const fn local(self) -> u32 {
                self.local
            }

            pub(crate) const fn index(self) -> usize {
                self.local as usize
            }
        }
    };
}

body_id!(ExprId);
body_id!(StmtId);
body_id!(PatternId);
body_id!(AxisExprId);
