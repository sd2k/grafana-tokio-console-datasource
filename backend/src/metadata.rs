#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq)]
pub struct MetaId(pub u64);

#[derive(Debug)]
pub struct Metadata {
    pub id: MetaId,
    pub field_names: Vec<String>,
    pub target: String,
    //TODO: add more metadata as needed
}

impl Metadata {
    pub fn from_proto(pb: console_api::Metadata, id: u64) -> Self {
        Self {
            id: MetaId(id),
            field_names: pb.field_names,
            target: pb.target,
        }
    }
}
