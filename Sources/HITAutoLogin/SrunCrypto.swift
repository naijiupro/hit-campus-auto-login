import CryptoKit
import Foundation

struct SrunLoginParameters {
    let action: String
    let username: String
    let password: String
    let acID: String
    let ip: String
    let checksum: String
    let info: String
    let n: String
    let type: String
    let os: String
    let name: String
    let doubleStack: String

    var queryItems: [URLQueryItem] {
        [
            URLQueryItem(name: "action", value: action),
            URLQueryItem(name: "username", value: username),
            URLQueryItem(name: "password", value: password),
            URLQueryItem(name: "ac_id", value: acID),
            URLQueryItem(name: "ip", value: ip),
            URLQueryItem(name: "chksum", value: checksum),
            URLQueryItem(name: "info", value: info),
            URLQueryItem(name: "n", value: n),
            URLQueryItem(name: "type", value: type),
            URLQueryItem(name: "os", value: os),
            URLQueryItem(name: "name", value: name),
            URLQueryItem(name: "double_stack", value: doubleStack)
        ]
    }
}

enum SrunCrypto {
    static let customBase64Alphabet = "LVoJPiCN2R8G90yg+hmFHuacZ1OWMnrsSTXkYpUq/3dlbfKwv6xztjI7DeBE45QA"
    private static let standardBase64Alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
    private static let encVersion = "srun_bx1"
    private static let n = "200"
    private static let type = "1"

    static func makeLoginParameters(
        username: String,
        password: String,
        ip: String,
        acID: String,
        token: String
    ) -> SrunLoginParameters {
        let hmd5 = hmacMD5(message: password, key: token)
        let infoJSON = makeInfoJSON(
            username: username,
            password: password,
            ip: ip,
            acID: acID
        )
        let encodedInfo = "{SRBX1}" + customBase64(xEncode(infoJSON, key: token))

        let checksumSource = token + username
            + token + hmd5
            + token + acID
            + token + ip
            + token + n
            + token + type
            + token + encodedInfo

        return SrunLoginParameters(
            action: "login",
            username: username,
            password: "{MD5}" + hmd5,
            acID: acID,
            ip: ip,
            checksum: sha1(checksumSource),
            info: encodedInfo,
            n: n,
            type: type,
            os: "Mac OS",
            name: "Macintosh",
            doubleStack: "0"
        )
    }

    static func hmacMD5(message: String, key: String) -> String {
        let symmetricKey = SymmetricKey(data: Data(key.utf8))
        let digest = HMAC<Insecure.MD5>.authenticationCode(
            for: Data(message.utf8),
            using: symmetricKey
        )
        return digest.map { String(format: "%02x", $0) }.joined()
    }

    static func sha1(_ text: String) -> String {
        let digest = Insecure.SHA1.hash(data: Data(text.utf8))
        return digest.map { String(format: "%02x", $0) }.joined()
    }

    static func makeInfoJSON(
        username: String,
        password: String,
        ip: String,
        acID: String
    ) -> String {
        "{\"username\":\(quotedJSON(username)),"
            + "\"password\":\(quotedJSON(password)),"
            + "\"ip\":\(quotedJSON(ip)),"
            + "\"acid\":\(quotedJSON(acID)),"
            + "\"enc_ver\":\(quotedJSON(encVersion))}"
    }

    static func xEncode(_ text: String, key: String) -> Data {
        guard !text.isEmpty else { return Data() }

        var values = packedWords(text, includeLength: true)
        var keyWords = packedWords(key, includeLength: false)
        while keyWords.count < 4 {
            keyWords.append(0)
        }

        let lastIndex = values.count - 1
        var z = values[lastIndex]
        var y = values[0]
        let delta: UInt32 = 0x9E37_79B9
        var rounds = 6 + 52 / values.count
        var sum: UInt32 = 0

        while rounds > 0 {
            rounds -= 1
            sum = sum &+ delta
            let e = (sum >> 2) & 3

            if lastIndex > 0 {
                for index in 0..<lastIndex {
                    y = values[index + 1]
                    var mixed = (z >> 5) ^ (y << 2)
                    mixed = mixed &+ (((y >> 3) ^ (z << 4)) ^ (sum ^ y))
                    mixed = mixed &+ (keyWords[Int((UInt32(index) & 3) ^ e)] ^ z)
                    values[index] = values[index] &+ mixed
                    z = values[index]
                }
            }

            y = values[0]
            var mixed = (z >> 5) ^ (y << 2)
            mixed = mixed &+ (((y >> 3) ^ (z << 4)) ^ (sum ^ y))
            mixed = mixed &+ (keyWords[Int((UInt32(lastIndex) & 3) ^ e)] ^ z)
            values[lastIndex] = values[lastIndex] &+ mixed
            z = values[lastIndex]
        }

        var output = Data()
        output.reserveCapacity(values.count * 4)
        for word in values {
            output.append(UInt8(truncatingIfNeeded: word))
            output.append(UInt8(truncatingIfNeeded: word >> 8))
            output.append(UInt8(truncatingIfNeeded: word >> 16))
            output.append(UInt8(truncatingIfNeeded: word >> 24))
        }
        return output
    }

    static func customBase64(_ data: Data) -> String {
        let standard = Array(standardBase64Alphabet)
        let custom = Array(customBase64Alphabet)
        return String(data.base64EncodedString().map { character in
            guard let index = standard.firstIndex(of: character) else {
                return character
            }
            return custom[index]
        })
    }

    private static func packedWords(_ text: String, includeLength: Bool) -> [UInt32] {
        // 对齐网页脚本的 JavaScript charCodeAt 语义（UTF-16 code units）。
        let codeUnits = Array(text.utf16)
        var words = [UInt32](repeating: 0, count: (codeUnits.count + 3) / 4)

        for (index, codeUnit) in codeUnits.enumerated() {
            words[index >> 2] |= UInt32(codeUnit) << UInt32((index & 3) * 8)
        }
        if includeLength {
            words.append(UInt32(codeUnits.count))
        }
        return words
    }

    private static func quotedJSON(_ value: String) -> String {
        guard let data = try? JSONSerialization.data(
            withJSONObject: value,
            options: [.fragmentsAllowed, .withoutEscapingSlashes]
        ), let quoted = String(data: data, encoding: .utf8) else {
            return "\"\""
        }
        return quoted
    }
}
