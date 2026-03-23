//! extra tls roots added by default.
//! currently will be used to fix example.com

use rustls_pki_types::{Der, TrustAnchor};

pub const EXTRA_TLS_SERVER_ROOTS: &[TrustAnchor<'static>] = &[
  /*
   * Issuer: CN=AAA Certificate Services O=Comodo CA Limited
   * Subject: CN=AAA Certificate Services O=Comodo CA Limited
   * Label: "AAA Certificate Services"
   * Serial: 1
   * SHA256 Fingerprint: d7:a7:a0:fb:5d:7e:27:31:d7:71:e9:48:4e:bc:de:f7:1d:5f:0c:3e:0a:29:48:78:2b:c8:3e:e0:ea:69:9e:f4
   * -----BEGIN CERTIFICATE-----
   * MIIEMjCCAxqgAwIBAgIBATANBgkqhkiG9w0BAQUFADB7MQswCQYDVQQGEwJHQjEb
   * MBkGA1UECAwSR3JlYXRlciBNYW5jaGVzdGVyMRAwDgYDVQQHDAdTYWxmb3JkMRow
   * GAYDVQQKDBFDb21vZG8gQ0EgTGltaXRlZDEhMB8GA1UEAwwYQUFBIENlcnRpZmlj
   * YXRlIFNlcnZpY2VzMB4XDTA0MDEwMTAwMDAwMFoXDTI4MTIzMTIzNTk1OVowezEL
   * MAkGA1UEBhMCR0IxGzAZBgNVBAgMEkdyZWF0ZXIgTWFuY2hlc3RlcjEQMA4GA1UE
   * BwwHU2FsZm9yZDEaMBgGA1UECgwRQ29tb2RvIENBIExpbWl0ZWQxITAfBgNVBAMM
   * GEFBQSBDZXJ0aWZpY2F0ZSBTZXJ2aWNlczCCASIwDQYJKoZIhvcNAQEBBQADggEP
   * ADCCAQoCggEBAL5AnfRu4ep2hxxNRUSOvkbIgwadwSr+GB+O5AL686tdUIoWMQua
   * BtDFcCLNSS1UY8y2bmhGC1Pqy0wkwLxyTurxFa70VJoSCsN6sjNg4tqJVfMiWPPe
   * 3M/vg4aijJRPn2jymJBGhCfHdr/jzDUsi14HZGWCwEiwqJH5YZ92IFCokcdmtet4
   * YgNW8IoaE+oxox6gmf049vYnMlhvB/VruPsUK6+3qszWY19zjNoFmag4qMsXeDZR
   * rOme9Hg6jc8P2ULimAyrL58OAd7vn5lJ8S3frHRNG5i1R8XlKdH5kBjHYpy+g8cm
   * ez6KJcfA3Z3mNWgQIJ2P2N7Sw4ScDV7oL8kCAwEAAaOBwDCBvTAdBgNVHQ4EFgQU
   * oBEKIz6W8Qfs4q8p74Klf9AwpLQwDgYDVR0PAQH/BAQDAgEGMA8GA1UdEwEB/wQF
   * MAMBAf8wewYDVR0fBHQwcjA4oDagNIYyaHR0cDovL2NybC5jb21vZG9jYS5jb20v
   * QUFBQ2VydGlmaWNhdGVTZXJ2aWNlcy5jcmwwNqA0oDKGMGh0dHA6Ly9jcmwuY29t
   * b2RvLm5ldC9BQUFDZXJ0aWZpY2F0ZVNlcnZpY2VzLmNybDANBgkqhkiG9w0BAQUF
   * AAOCAQEACFb8AvCb6P+k+tZ7xkSAzk/ExfYAWMymtrwUSWgEdujm7l3sAg9g1o1Q
   * GE8mTgHj5rCl7r+8dFRBv/38ErjHT1r0iWAFf2C3BUrz9vHCv8S5dIa2LX1rzNLz
   * Rt0vxuBqw8M0Ayx9lt1awg6nCpnBBYurDC/zXDrPbDdVCYfeU0BsWO/8tqtlbgT2
   * G9w84FoVxp7Z8VlIMCFlA2zs6SFz7JsDoeA3raAVGI/6ugLOpyypEBMs1OUIJqsi
   * l2D4kF501KKaU73yqWjgom7C12yxow+ev+to51byrvLjKzg6CYG1a4XXvi3tPxq3
   * smPi9WIsgtRqAEFQ8TmDn5XpNpaYbg==
   * -----END CERTIFICATE-----
   */
  TrustAnchor {
    subject: Der::from_slice(b"1\x0b0\t\x06\x03U\x04\x06\x13\x02GB1\x1b0\x19\x06\x03U\x04\x08\x0c\x12Greater Manchester1\x100\x0e\x06\x03U\x04\x07\x0c\x07Salford1\x1a0\x18\x06\x03U\x04\n\x0c\x11Comodo CA Limited1!0\x1f\x06\x03U\x04\x03\x0c\x18AAA Certificate Services"),
    subject_public_key_info: Der::from_slice(b"0\r\x06\t*\x86H\x86\xf7\r\x01\x01\x01\x05\x00\x03\x82\x01\x0f\x000\x82\x01\n\x02\x82\x01\x01\x00\xbe@\x9d\xf4n\xe1\xeav\x87\x1cMED\x8e\xbeF\xc8\x83\x06\x9d\xc1*\xfe\x18\x1f\x8e\xe4\x02\xfa\xf3\xab]P\x8a\x161\x0b\x9a\x06\xd0\xc5p\"\xcdI-Tc\xcc\xb6nhF\x0bS\xea\xcbL$\xc0\xbcrN\xea\xf1\x15\xae\xf4T\x9a\x12\n\xc3z\xb23`\xe2\xda\x89U\xf3\"X\xf3\xde\xdc\xcf\xef\x83\x86\xa2\x8c\x94O\x9fh\xf2\x98\x90F\x84\'\xc7v\xbf\xe3\xcc5,\x8b^\x07de\x82\xc0H\xb0\xa8\x91\xf9a\x9fv P\xa8\x91\xc7f\xb5\xebxb\x03V\xf0\x8a\x1a\x13\xea1\xa3\x1e\xa0\x99\xfd8\xf6\xf6\'2Xo\x07\xf5k\xb8\xfb\x14+\xaf\xb7\xaa\xcc\xd6c_s\x8c\xda\x05\x99\xa88\xa8\xcb\x17x6Q\xac\xe9\x9e\xf4x:\x8d\xcf\x0f\xd9B\xe2\x98\x0c\xab/\x9f\x0e\x01\xde\xef\x9f\x99I\xf1-\xdf\xactM\x1b\x98\xb5G\xc5\xe5)\xd1\xf9\x90\x18\xc7b\x9c\xbe\x83\xc7&{>\x8a%\xc7\xc0\xdd\x9d\xe65h\x10 \x9d\x8f\xd8\xde\xd2\xc3\x84\x9c\r^\xe8/\xc9\x02\x03\x01\x00\x01"),
    name_constraints: None
  },
];
