//! CertificateSigningRequest approval rules.
//!
//! Approving a CSR lets its signer issue a certificate for whatever identity
//! the PKCS#10 request names: the subject's CN becomes the Kubernetes user
//! name and every O becomes a group. A request for `O=system:masters` signed
//! by `kubernetes.io/kube-apiserver-client` is a cluster-admin credential that
//! RBAC can't revoke, and `system:node:*`, `system:kube-*` or
//! `system:serviceaccount:*` names impersonate cluster components. So
//! `approve_csr` reads the request and refuses privileged identities unless
//! the operator opts in with `--allow-privileged-csr`.

use k8s_openapi::api::certificates::v1::CertificateSigningRequest;
use serde::Serialize;
use x509_cert::der::{asn1::Any, Decode};
use x509_cert::ext::pkix::name::DirectoryString;
use x509_cert::request::CertReq;

use super::{flags, Remedy, Violation, WorkloadPolicy};

/// The group that bypasses RBAC entirely.
const SYSTEM_MASTERS: &str = "system:masters";
/// Prefix Kubernetes reserves for users and groups of the system itself.
const SYSTEM_PREFIX: &str = "system:";
/// Signer for kubelet serving certificates.
const KUBELET_SERVING_SIGNER: &str = "kubernetes.io/kubelet-serving";

const OID_COMMON_NAME: &str = "2.5.4.3";
const OID_ORGANIZATION: &str = "2.5.4.10";

/// The Kubernetes identity a PKCS#10 request asks for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CsrIdentity {
    /// Subject common names (the user name). Kubernetes uses one; every value
    /// present is reported and checked.
    pub common_names: Vec<String>,
    /// Subject organizations (the groups).
    pub organizations: Vec<String>,
}

/// Parse the PEM `spec.request` of a CSR (one `CERTIFICATE REQUEST` block
/// holding a DER PKCS#10 request) and return its subject's CN and O values.
pub fn parse_csr_identity(request: &[u8]) -> Result<CsrIdentity, String> {
    let blocks = pem::parse_many(request).map_err(|e| format!("invalid PEM: {e}"))?;
    let block = match blocks.as_slice() {
        [block] => block,
        [] => return Err("no PEM block found".into()),
        _ => return Err(format!("expected one PEM block, found {}", blocks.len())),
    };
    if block.tag() != "CERTIFICATE REQUEST" {
        return Err(format!(
            "expected a CERTIFICATE REQUEST PEM block, found {:?}",
            block.tag()
        ));
    }
    let req =
        CertReq::from_der(block.contents()).map_err(|e| format!("invalid PKCS#10 request: {e}"))?;

    let mut identity = CsrIdentity {
        common_names: Vec::new(),
        organizations: Vec::new(),
    };
    for atv in req.info.subject.iter() {
        let target = match atv.oid.to_string().as_str() {
            OID_COMMON_NAME => &mut identity.common_names,
            OID_ORGANIZATION => &mut identity.organizations,
            _ => continue,
        };
        target.push(directory_string(&atv.value)?);
    }
    Ok(identity)
}

fn directory_string(value: &Any) -> Result<String, String> {
    DirectoryString::try_from(value)
        .map(String::from)
        .map_err(|e| format!("unreadable subject attribute: {e}"))
}

fn privileged(location: &str, reason: String) -> Violation {
    Violation::new(
        location,
        reason,
        Remedy::Enable(&flags::ALLOW_PRIVILEGED_CSR),
    )
}

/// Check whether approving `csr` would mint a privileged identity.
///
/// Unless [`WorkloadPolicy::allow_privileged_csr`] is set, this rejects:
/// - a request that isn't exactly one parseable `CERTIFICATE REQUEST` PEM block;
/// - the group `system:masters`, for every signer;
/// - any CN or O starting with `system:` (nodes, control-plane components,
///   service accounts, `system:anonymous`, `system:unauthenticated`, …), for
///   every signer. The one exception is a node asking for its own serving
///   certificate: signer `kubernetes.io/kubelet-serving`, subject
///   `CN=system:node:<name>, O=system:nodes`, requested by `system:node:<name>`.
///   (Its SANs aren't checked against the node's addresses.)
pub fn check_csr(
    policy: &WorkloadPolicy,
    csr: &CertificateSigningRequest,
    out: &mut Vec<Violation>,
) {
    if policy.allow_privileged_csr {
        return;
    }
    let identity = match parse_csr_identity(&csr.spec.request.0) {
        Ok(identity) => identity,
        Err(e) => {
            out.push(privileged(
                "spec.request",
                format!("can't read the requested identity ({e}), so it can't be checked"),
            ));
            return;
        }
    };

    if identity.organizations.iter().any(|o| o == SYSTEM_MASTERS) {
        out.push(privileged(
            "spec.request (subject O)",
            "requests group system:masters, which bypasses RBAC: the certificate would be a cluster-admin credential that can't be revoked".into(),
        ));
    }

    if is_own_kubelet_serving_cert(csr, &identity) {
        return;
    }
    for cn in identity
        .common_names
        .iter()
        .filter(|cn| cn.starts_with(SYSTEM_PREFIX))
    {
        out.push(privileged(
            "spec.request (subject CN)",
            format!(
                "requests user {cn:?}: system: names belong to cluster components, nodes and service accounts"
            ),
        ));
    }
    for o in identity
        .organizations
        .iter()
        .filter(|o| o.starts_with(SYSTEM_PREFIX) && *o != SYSTEM_MASTERS)
    {
        out.push(privileged(
            "spec.request (subject O)",
            format!("requests group {o:?}: system: groups belong to cluster components, nodes and service accounts"),
        ));
    }
}

/// A node requesting a serving certificate for itself: what kubelet serving
/// certificate rotation creates, and what clusters without an auto-approver
/// approve by hand.
fn is_own_kubelet_serving_cert(csr: &CertificateSigningRequest, identity: &CsrIdentity) -> bool {
    let [cn] = identity.common_names.as_slice() else {
        return false;
    };
    csr.spec.signer_name == KUBELET_SERVING_SIGNER
        && cn
            .strip_prefix("system:node:")
            .is_some_and(|node| !node.is_empty())
        && identity.organizations == ["system:nodes"]
        && csr.spec.username.as_deref() == Some(cn.as_str())
}
