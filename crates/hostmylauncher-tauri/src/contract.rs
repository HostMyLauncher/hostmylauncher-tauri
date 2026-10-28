//! Transport-only types. Optional fields serialize as explicit nulls.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

macro_rules! dto {
    ($name:ident { $($field:ident : $ty:ty $(=> $ts:literal)?),* $(,)? }) => {
        #[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        pub struct $name {
            $( $(#[ts(type = $ts)])? pub $field: $ty, )*
        }
    };
}
macro_rules! wire_enum {
    ($name:ident { $($variant:ident),* $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "snake_case")]
        pub enum $name { $($variant),* }
    };
}

dto!(ListInstancesRequest { launcher_id: String, limit: Option<u32>, offset: Option<u32> });
dto!(GetInstanceRequest {
    launcher_id: String,
    instance_id: String
});
// Secret-bearing inputs deliberately have no Debug implementation.
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ResolveByCodeRequest {
    pub launcher_ids: Vec<String>,
    pub code: String,
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GetInstanceFilesRequest {
    pub launcher_id: String,
    pub instance_id: String,
    pub code: Option<String>,
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct StartOperationRequest {
    pub launcher_id: String,
    pub instance_id: String,
    pub kind: OperationKind,
    pub code: Option<String>,
}
dto!(GetOperationRequest {
    operation_id: String
});
dto!(LoaderDto { kind: Option<String>, build: Option<String>, enabled: bool });
dto!(InstanceDto {
    id: String, launcher_id: Option<String>, slug: Option<String>, title: Option<String>,
    minecraft_version: Option<String>, server: Option<String>, loader: Option<LoaderDto>,
    java_version: Option<u32>, suggested_ram: Option<u32>, icon: Option<String>,
    landscape: Option<String>, poster: Option<String>, animation: Option<String>,
    locked: bool, downloads: u64 => "number"
});
dto!(HashesDto { sha1: Option<String>, sha512: Option<String> });
dto!(FileDescriptorDto {
    path: Option<String>, name: Option<String>, size: Option<u64> => "number | null",
    hashes: Option<HashesDto>
});
wire_enum!(OperationKind {
    Sync,
    Verify,
    Repair
});
wire_enum!(OperationStatus {
    Queued,
    Running,
    Succeeded,
    Failed
});
wire_enum!(ProgressStage {
    FetchingFiles,
    Download,
    Extract,
    Install,
    Remove,
    Verify
});
wire_enum!(ProgressUnit {
    Bytes,
    Entries,
    Archives
});
dto!(OperationProgress {
    stage: ProgressStage, current: Option<u64> => "number | null",
    total: Option<u64> => "number | null", unit: Option<ProgressUnit>
});
wire_enum!(VerificationIssueKind { Missing, Different });
dto!(VerificationIssue {
    path: String,
    kind: VerificationIssueKind
});
dto!(VerificationReport { checked: u64 => "number", issues: Vec<VerificationIssue> });
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum OperationResult {
    Sync {
        #[ts(type = "number")]
        downloaded: u64,
        #[ts(type = "number")]
        removed: u64,
    },
    Verify {
        #[ts(type = "number")]
        checked: u64,
        issues: Vec<VerificationIssue>,
    },
    Repair {
        verification: VerificationReport,
        #[ts(type = "number")]
        repaired: u64,
        #[ts(type = "number")]
        downloaded: u64,
    },
}
wire_enum!(ErrorCode {
    InvalidRequest,
    NotFound,
    Unauthorized,
    Api,
    Network,
    Filesystem,
    Decode,
    OperationConflict,
    Internal
});
dto!(ErrorContext { http_status: u16 });
dto!(BridgeError {
    code: ErrorCode, operation_id: Option<String>, stage: Option<ProgressStage>,
    context: Option<ErrorContext>
});
dto!(OperationSnapshot {
    operation_id: String, launcher_id: String, instance_id: String, kind: OperationKind, status: OperationStatus,
    sequence: u64 => "number", progress: Option<OperationProgress>,
    result: Option<OperationResult>, error: Option<BridgeError>
});

/// A deterministic single module, generated from derives rather than DTO copies.
pub fn typescript_types() -> String {
    let mut output = String::new();
    macro_rules! append {
        ($($ty:ty),* $(,)?) => { $(
            output.push_str("export ");
            output.push_str(&<$ty as TS>::decl());
            output.push('\n');
        )* };
    }
    append!(
        ListInstancesRequest,
        GetInstanceRequest,
        ResolveByCodeRequest,
        GetInstanceFilesRequest,
        StartOperationRequest,
        GetOperationRequest,
        LoaderDto,
        InstanceDto,
        HashesDto,
        FileDescriptorDto,
        OperationKind,
        OperationStatus,
        ProgressStage,
        ProgressUnit,
        OperationProgress,
        VerificationIssueKind,
        VerificationIssue,
        VerificationReport,
        OperationResult,
        ErrorCode,
        ErrorContext,
        BridgeError,
        OperationSnapshot
    );
    output
}
