//! PKCS#10 requests for the CSR approval tests, generated with
//! `openssl req -new -key <P-256 key> -subj <subject>`.

/// `CN=mallory, O=system:masters`
pub const MASTERS: &str = "-----BEGIN CERTIFICATE REQUEST-----
MIHlMIGNAgEAMCsxEDAOBgNVBAMMB21hbGxvcnkxFzAVBgNVBAoMDnN5c3RlbTpt
YXN0ZXJzMFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEZYwjcLOvBBYNFb1UwJoe
OXK1ni6WKdLieX716XuP1uuFy5kWoDJVYnIXm6nVsRDssOogvbHRHWMsxdmGhMp8
KaAAMAoGCCqGSM49BAMCA0cAMEQCIGHFtUfqC9chXD4xopDjC/u5mNM4JoP6IbdU
VpiV2dmxAiB6JOsjfZkqVST19VCnIlPP/Mo0sCJdVPIajfeNRpAl/g==
-----END CERTIFICATE REQUEST-----
";

/// `CN=eve, O=dev, O=system:masters` (system:masters hidden in a later RDN)
pub const MASTERS_SECOND_O: &str = "-----BEGIN CERTIFICATE REQUEST-----
MIHwMIGXAgEAMDUxDDAKBgNVBAMMA2V2ZTEMMAoGA1UECgwDZGV2MRcwFQYDVQQK
DA5zeXN0ZW06bWFzdGVyczBZMBMGByqGSM49AgEGCCqGSM49AwEHA0IABGWMI3Cz
rwQWDRW9VMCaHjlytZ4ulinS4nl+9el7j9brhcuZFqAyVWJyF5up1bEQ7LDqIL2x
0R1jLMXZhoTKfCmgADAKBggqhkjOPQQDAgNIADBFAiEAzBlUNiMbdywBwJvg49BI
uc4QCZFGRHzRafdpkxxDPtMCIEP16XvcVwQzpM1Aax6bD6vWrOcq4koQKsO862av
XTKy
-----END CERTIFICATE REQUEST-----
";

/// `CN=jane, O=dev, O=qa`
pub const USER: &str = "-----BEGIN CERTIFICATE REQUEST-----
MIHkMIGMAgEAMCoxDTALBgNVBAMMBGphbmUxDDAKBgNVBAoMA2RldjELMAkGA1UE
CgwCcWEwWTATBgcqhkjOPQIBBggqhkjOPQMBBwNCAARljCNws68EFg0VvVTAmh45
crWeLpYp0uJ5fvXpe4/W64XLmRagMlVichebqdWxEOyw6iC9sdEdYyzF2YaEynwp
oAAwCgYIKoZIzj0EAwIDRwAwRAIgQGOGTk3prjgv/RPUXe6g+l+QE8RcJnlh0XFy
4GDE9hECIDH3pBEOPCawcSPTBxjGZRCUXKpWJPhUSr5PuR39i9uy
-----END CERTIFICATE REQUEST-----
";

/// `CN=system:node:node-1, O=system:nodes`
pub const NODE: &str = "-----BEGIN CERTIFICATE REQUEST-----
MIHuMIGWAgEAMDQxGzAZBgNVBAMMEnN5c3RlbTpub2RlOm5vZGUtMTEVMBMGA1UE
CgwMc3lzdGVtOm5vZGVzMFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEZYwjcLOv
BBYNFb1UwJoeOXK1ni6WKdLieX716XuP1uuFy5kWoDJVYnIXm6nVsRDssOogvbHR
HWMsxdmGhMp8KaAAMAoGCCqGSM49BAMCA0cAMEQCIBOftsoAjxYK06dSPR7iGebQ
hkXvD+4cRTKt+HXMX3T2AiBZpiobwp/8TOcdoWk7LTVE+Q5CtfBs40p9PtaFfZH9
GQ==
-----END CERTIFICATE REQUEST-----
";

/// `CN=system:anonymous`
pub const ANONYMOUS: &str = "-----BEGIN CERTIFICATE REQUEST-----
MIHVMH0CAQAwGzEZMBcGA1UEAwwQc3lzdGVtOmFub255bW91czBZMBMGByqGSM49
AgEGCCqGSM49AwEHA0IABGWMI3CzrwQWDRW9VMCaHjlytZ4ulinS4nl+9el7j9br
hcuZFqAyVWJyF5up1bEQ7LDqIL2x0R1jLMXZhoTKfCmgADAKBggqhkjOPQQDAgNI
ADBFAiEA2xoM7a4wD0uZYZ49jNtB/pqqtFzo6I7C1YWkzKz7llMCIBh+H4l3SEVj
PeMcAW9CmwBnAwiBIJ3i8TieJPHiPugU
-----END CERTIFICATE REQUEST-----
";

/// `CN=bob, O=system:serviceaccounts`
pub const SERVICEACCOUNTS_GROUP: &str = "-----BEGIN CERTIFICATE REQUEST-----
MIHrMIGRAgEAMC8xDDAKBgNVBAMMA2JvYjEfMB0GA1UECgwWc3lzdGVtOnNlcnZp
Y2VhY2NvdW50czBZMBMGByqGSM49AgEGCCqGSM49AwEHA0IABGWMI3CzrwQWDRW9
VMCaHjlytZ4ulinS4nl+9el7j9brhcuZFqAyVWJyF5up1bEQ7LDqIL2x0R1jLMXZ
hoTKfCmgADAKBggqhkjOPQQDAgNJADBGAiEAmKlBXEmEZv2rA7OA/dHwwxNF4ti+
AmhULZA9cb+afBoCIQDd00dfLQ/UFFkKAaXr2wlovkq1sLJf5j566Kq6M3b4qQ==
-----END CERTIFICATE REQUEST-----
";

/// A CertificateSigningRequest with the given PEM request, signer and requester.
pub fn csr(
    request: &str,
    signer: &str,
    username: Option<&str>,
) -> k8s_openapi::api::certificates::v1::CertificateSigningRequest {
    use k8s_openapi::api::certificates::v1::{
        CertificateSigningRequest, CertificateSigningRequestSpec,
    };
    CertificateSigningRequest {
        metadata: Default::default(),
        spec: CertificateSigningRequestSpec {
            request: k8s_openapi::ByteString(request.as_bytes().to_vec()),
            signer_name: signer.to_string(),
            username: username.map(str::to_string),
            usages: Some(vec!["client auth".to_string()]),
            ..Default::default()
        },
        status: None,
    }
}
