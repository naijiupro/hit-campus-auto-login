import Foundation
import XCTest
@testable import HITAutoLogin

final class HITAutoLoginTests: XCTestCase {
    func testHMACMD5MatchesKnownVector() {
        XCTAssertEqual(
            SrunCrypto.hmacMD5(
                message: "The quick brown fox jumps over the lazy dog",
                key: "key"
            ),
            "80070713463e7749b90c2dc24911e275"
        )
    }

    func testSHA1MatchesKnownVector() {
        XCTAssertEqual(
            SrunCrypto.sha1("The quick brown fox jumps over the lazy dog"),
            "2fd4e1c67a2d28fced849ee1bb76e7391b93eb12"
        )
    }

    func testCompleteSrunParametersMatchJavaScriptReference() {
        let parameters = SrunCrypto.makeLoginParameters(
            username: "2024000000",
            password: "password",
            ip: "10.0.0.42",
            acID: "27",
            token: "0123456789abcdef0123456789abcdef"
        )

        XCTAssertEqual(parameters.password, "{MD5}d317550f8126002512bb01d794c9ffa2")
        XCTAssertEqual(
            parameters.info,
            "{SRBX1}W+FIdBHb99ePoYgDyr5/mdNfPyeR8eEmbRC0h0YvrwE85XNO0BKRwwKSN8I2mQNNXdv1KZ/pq6RHfcDBkEpR+do5TMf2oZZjWENME+3DjWqfsRAa1QgJVl4U60eRctwlVzkYOP0Sm6S="
        )
        XCTAssertEqual(parameters.checksum, "923eb2c3ac4a31fd825d66a6a38d6b4a6fc79370")
    }

    func testSrunEncodingMatchesJavaScriptForUnicodePassword() {
        let parameters = SrunCrypto.makeLoginParameters(
            username: "2024000000",
            password: "密码🔐",
            ip: "10.0.0.42",
            acID: "27",
            token: "0123456789abcdef0123456789abcdef"
        )

        XCTAssertEqual(parameters.password, "{MD5}52c2d750075c768ad3d5811af10df80e")
        XCTAssertEqual(
            parameters.info,
            "{SRBX1}+Yd2acmMz0EhMm18ImhS3yh2JsNR2JDBsm0PWnxI7gircMegasQYAsV65OnYxZObRMnLOSEP0MHCvqfaSezJDE2B1cAVXqTEQnfbS8InAAPMyZkj490aDNJyekb3frQ2cBggjS=="
        )
        XCTAssertEqual(parameters.checksum, "5aeb46c1c6ef126fe3acd4452d4131916d48c4d8")
    }

    func testPortalPageParserUsesLiveFields() {
        let html = """
        <input value="27" type="hidden" id="ac_id">
        <input id='user_ip' name='user_ip' value='10.0.0.42'>
        """
        XCTAssertEqual(
            PortalPageParser.parse(html),
            PortalFields(acID: "27", userIP: "10.0.0.42")
        )
    }

    func testJSONPParserAcceptsCallbackWrapper() throws {
        let data = Data("callback_1({\"error\":\"ok\",\"challenge\":\"abc\"})".utf8)
        let object = try JSONPParser.parse(data)
        XCTAssertEqual(object["error"] as? String, "ok")
        XCTAssertEqual(object["challenge"] as? String, "abc")
    }
}
