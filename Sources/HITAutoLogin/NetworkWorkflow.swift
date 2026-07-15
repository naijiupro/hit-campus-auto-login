import Foundation

struct AutoLoginConfiguration {
    let username: String
    let password: String
    let ssid: String
}

struct WorkflowResult {
    let message: String
}

enum AutoLoginError: LocalizedError {
    case commandFailed(String)
    case wifiInterfaceNotFound
    case wifiConnectionFailed(String)
    case portalPageInvalid
    case invalidPortalResponse
    case challengeFailed(String)
    case authenticationFailed(String)
    case verificationFailed

    var errorDescription: String? {
        switch self {
        case .commandFailed(let message):
            return "系统命令执行失败：\(message)"
        case .wifiInterfaceNotFound:
            return "没有找到 Mac 的 Wi‑Fi 网络接口"
        case .wifiConnectionFailed(let message):
            return "无法连接 HIT-WLAN：\(message)"
        case .portalPageInvalid:
            return "认证门户返回了无法识别的页面"
        case .invalidPortalResponse:
            return "认证门户返回了无法解析的数据"
        case .challengeFailed(let message):
            return "获取认证挑战值失败：\(message)"
        case .authenticationFailed(let message):
            return "校园网认证失败：\(message)"
        case .verificationFailed:
            return "认证请求已完成，但仍无法访问互联网"
        }
    }
}

struct NetworkWorkflow {
    private let wifi = WiFiManager()

    func execute(
        configuration: AutoLoginConfiguration,
        progress: @escaping @MainActor (String) -> Void
    ) async throws -> WorkflowResult {
        // 新版 macOS 在未授予定位权限时可能隐藏当前 SSID，哪怕 Wi‑Fi
        // 已正常联网。因此先验证实际结果，避免把可用网络误判为未连接。
        await progress("正在检测互联网连接…")
        if await ConnectivityChecker.isOnline() {
            return WorkflowResult(message: "网络已经连通，无需重复认证")
        }

        await progress("正在检查 Wi‑Fi…")
        let interface = try await wifi.wifiInterface()
        let currentSSID = try await wifi.currentSSID(interface: interface)

        if currentSSID != configuration.ssid {
            await progress("正在连接 \(configuration.ssid)…")
            try await wifi.connect(to: configuration.ssid, interface: interface)
        }

        await progress("Wi‑Fi 已就绪，正在检测互联网…")
        if await ConnectivityChecker.isOnline() {
            return WorkflowResult(message: "网络已经连通，无需重复认证")
        }

        await progress("正在向 HIT 门户认证…")
        let portalMessage = try await PortalClient().authenticate(
            username: configuration.username,
            password: configuration.password
        )

        await progress("认证完成，正在验证网络…")
        for attempt in 0..<3 {
            if attempt > 0 {
                try await Task.sleep(nanoseconds: 1_500_000_000)
            }
            if await ConnectivityChecker.isOnline() {
                let suffix = portalMessage.isEmpty ? "" : "（\(portalMessage)）"
                return WorkflowResult(message: "认证成功，网络已连通\(suffix)")
            }
        }

        throw AutoLoginError.verificationFailed
    }
}

private struct CommandResult {
    let status: Int32
    let output: String
}

private enum CommandRunner {
    static func run(_ executable: String, arguments: [String]) async throws -> CommandResult {
        try await withCheckedThrowingContinuation { continuation in
            let process = Process()
            let outputPipe = Pipe()
            process.executableURL = URL(fileURLWithPath: executable)
            process.arguments = arguments
            process.standardOutput = outputPipe
            process.standardError = outputPipe
            process.terminationHandler = { finishedProcess in
                let data = outputPipe.fileHandleForReading.readDataToEndOfFile()
                let output = String(data: data, encoding: .utf8) ?? ""
                continuation.resume(returning: CommandResult(
                    status: finishedProcess.terminationStatus,
                    output: output.trimmingCharacters(in: .whitespacesAndNewlines)
                ))
            }

            do {
                try process.run()
            } catch {
                continuation.resume(throwing: error)
            }
        }
    }
}

private struct WiFiManager {
    private let networksetup = "/usr/sbin/networksetup"

    func wifiInterface() async throws -> String {
        let result = try await CommandRunner.run(networksetup, arguments: ["-listallhardwareports"])
        guard result.status == 0 else {
            throw AutoLoginError.commandFailed(result.output)
        }

        let lines = result.output.components(separatedBy: .newlines)
        for (index, line) in lines.enumerated() {
            let isWiFi = line == "Hardware Port: Wi-Fi" || line == "Hardware Port: AirPort"
            guard isWiFi else { continue }

            for candidate in lines.dropFirst(index + 1).prefix(3) {
                if candidate.hasPrefix("Device: ") {
                    return String(candidate.dropFirst("Device: ".count))
                }
            }
        }
        throw AutoLoginError.wifiInterfaceNotFound
    }

    func currentSSID(interface: String) async throws -> String? {
        let result = try await CommandRunner.run(
            networksetup,
            arguments: ["-getairportnetwork", interface]
        )
        guard result.status == 0 else { return nil }

        let prefix = "Current Wi-Fi Network: "
        guard let range = result.output.range(of: prefix) else { return nil }
        let ssid = String(result.output[range.upperBound...]).trimmingCharacters(in: .whitespacesAndNewlines)
        return ssid.isEmpty ? nil : ssid
    }

    func connect(to ssid: String, interface: String) async throws {
        let powerResult = try await CommandRunner.run(
            networksetup,
            arguments: ["-setairportpower", interface, "on"]
        )
        guard powerResult.status == 0 else {
            throw AutoLoginError.wifiConnectionFailed(
                powerResult.output.isEmpty ? "无法开启 Wi‑Fi" : powerResult.output
            )
        }

        let result = try await CommandRunner.run(
            networksetup,
            arguments: ["-setairportnetwork", interface, ssid]
        )
        let lowercasedOutput = result.output.lowercased()
        guard result.status == 0,
              !lowercasedOutput.contains("error"),
              !lowercasedOutput.contains("failed") else {
            throw AutoLoginError.wifiConnectionFailed(
                result.output.isEmpty ? "networksetup 返回状态 \(result.status)" : result.output
            )
        }

        // `-getairportnetwork` 在 macOS 15+ 可能始终返回“未关联”，不能再用
        // 它作为连接成功的硬性判据。setairportnetwork 成功后短暂等待 DHCP，
        // 后续由公网检测和门户请求给出真正的结果。
        try await Task.sleep(nanoseconds: 2_000_000_000)
    }
}

private enum ConnectivityChecker {
    static func isOnline() async -> Bool {
        if let ping = try? await CommandRunner.run(
            "/sbin/ping",
            arguments: ["-c", "1", "-W", "2000", "baidu.com"]
        ), ping.status == 0 {
            return true
        }

        // 部分网络禁用 ICMP，因此用 HTTPS 做一次补充验证，并拒绝门户重定向。
        let configuration = URLSessionConfiguration.ephemeral
        configuration.timeoutIntervalForRequest = 5
        configuration.timeoutIntervalForResource = 6
        let session = URLSession(configuration: configuration)
        var request = URLRequest(url: URL(string: "https://www.baidu.com/robots.txt")!)
        request.cachePolicy = .reloadIgnoringLocalAndRemoteCacheData

        do {
            let (_, response) = try await session.data(for: request)
            guard let httpResponse = response as? HTTPURLResponse,
                  (200..<500).contains(httpResponse.statusCode),
                  let host = httpResponse.url?.host?.lowercased() else {
                return false
            }
            return host == "baidu.com" || host.hasSuffix(".baidu.com")
        } catch {
            return false
        }
    }
}

struct PortalFields: Equatable {
    let acID: String
    let userIP: String
}

enum PortalPageParser {
    static func parse(_ html: String) -> PortalFields {
        PortalFields(
            acID: inputValue(id: "ac_id", in: html) ?? "27",
            userIP: inputValue(id: "user_ip", in: html) ?? ""
        )
    }

    private static func inputValue(id: String, in html: String) -> String? {
        let escapedID = NSRegularExpression.escapedPattern(for: id)
        let tagPattern = "<input\\b(?=[^>]*\\bid\\s*=\\s*[\"']\(escapedID)[\"'])[^>]*>"
        guard let tagRegex = try? NSRegularExpression(
            pattern: tagPattern,
            options: [.caseInsensitive]
        ) else { return nil }

        let htmlRange = NSRange(html.startIndex..<html.endIndex, in: html)
        guard let tagMatch = tagRegex.firstMatch(in: html, range: htmlRange),
              let tagRange = Range(tagMatch.range, in: html) else {
            return nil
        }

        let tag = String(html[tagRange])
        let valuePattern = "\\bvalue\\s*=\\s*[\"']([^\"']*)[\"']"
        guard let valueRegex = try? NSRegularExpression(
            pattern: valuePattern,
            options: [.caseInsensitive]
        ) else { return nil }

        let range = NSRange(tag.startIndex..<tag.endIndex, in: tag)
        guard let valueMatch = valueRegex.firstMatch(in: tag, range: range),
              valueMatch.numberOfRanges > 1,
              let valueRange = Range(valueMatch.range(at: 1), in: tag) else {
            return nil
        }
        return String(tag[valueRange])
    }
}

enum JSONPParser {
    static func parse(_ data: Data) throws -> [String: Any] {
        guard let text = String(data: data, encoding: .utf8),
              let start = text.firstIndex(of: "{"),
              let end = text.lastIndex(of: "}"),
              start <= end else {
            throw AutoLoginError.invalidPortalResponse
        }

        let jsonText = String(text[start...end])
        guard let jsonData = jsonText.data(using: .utf8),
              let object = try JSONSerialization.jsonObject(with: jsonData) as? [String: Any] else {
            throw AutoLoginError.invalidPortalResponse
        }
        return object
    }
}

private final class PortalClient {
    private let baseURL = URL(string: "https://wp.hit.edu.cn")!
    private let session: URLSession

    init() {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.timeoutIntervalForRequest = 8
        configuration.timeoutIntervalForResource = 12
        session = URLSession(configuration: configuration)
    }

    func authenticate(username: String, password: String) async throws -> String {
        let fields = try await loadPortalFields()
        let challenge = try await requestJSONP(
            path: "/cgi-bin/get_challenge",
            queryItems: [
                URLQueryItem(name: "username", value: username),
                URLQueryItem(name: "ip", value: fields.userIP)
            ]
        )

        let challengeError = stringValue(challenge["error"])
        guard challengeError == "ok",
              let token = nonemptyString(challenge["challenge"]) else {
            let message = nonemptyString(challenge["error_msg"])
                ?? nonemptyString(challenge["ecode"])
                ?? challengeError
            throw AutoLoginError.challengeFailed(message.isEmpty ? "未知错误" : message)
        }

        let clientIP = fields.userIP.isEmpty
            ? (nonemptyString(challenge["client_ip"]) ?? "")
            : fields.userIP
        guard !clientIP.isEmpty else {
            throw AutoLoginError.portalPageInvalid
        }

        let parameters = SrunCrypto.makeLoginParameters(
            username: username,
            password: password,
            ip: clientIP,
            acID: fields.acID,
            token: token
        )
        let response = try await requestJSONP(
            path: "/cgi-bin/srun_portal",
            queryItems: parameters.queryItems
        )

        let error = stringValue(response["error"])
        let successMessage = nonemptyString(response["ploy_msg"])
            ?? nonemptyString(response["suc_msg"])
            ?? ""
        if error == "ok" {
            if successMessage.hasPrefix("E0000") {
                return ""
            }
            return successMessage
        }

        let message = nonemptyString(response["ploy_msg"])
            ?? nonemptyString(response["error_msg"])
            ?? nonemptyString(response["suc_msg"])
            ?? nonemptyString(response["ecode"])
            ?? (error.isEmpty ? "未知错误" : error)

        if message.lowercased().contains("already_online") {
            return "当前 IP 已在线"
        }
        throw AutoLoginError.authenticationFailed(message)
    }

    private func loadPortalFields() async throws -> PortalFields {
        var request = URLRequest(url: baseURL)
        request.cachePolicy = .reloadIgnoringLocalAndRemoteCacheData
        let (data, response) = try await session.data(for: request)
        guard let httpResponse = response as? HTTPURLResponse,
              (200..<400).contains(httpResponse.statusCode),
              let html = String(data: data, encoding: .utf8) else {
            throw AutoLoginError.portalPageInvalid
        }
        return PortalPageParser.parse(html)
    }

    private func requestJSONP(
        path: String,
        queryItems: [URLQueryItem]
    ) async throws -> [String: Any] {
        guard var components = URLComponents(
            url: baseURL.appendingPathComponent(path),
            resolvingAgainstBaseURL: false
        ) else {
            throw AutoLoginError.invalidPortalResponse
        }

        let timestamp = String(Int(Date().timeIntervalSince1970 * 1_000))
        components.queryItems = [URLQueryItem(name: "callback", value: "HITAutoLogin_\(timestamp)")]
            + queryItems
            + [URLQueryItem(name: "_", value: timestamp)]
        guard let url = components.url else {
            throw AutoLoginError.invalidPortalResponse
        }

        var request = URLRequest(url: url)
        request.cachePolicy = .reloadIgnoringLocalAndRemoteCacheData
        let (data, response) = try await session.data(for: request)
        guard let httpResponse = response as? HTTPURLResponse,
              (200..<400).contains(httpResponse.statusCode) else {
            throw AutoLoginError.invalidPortalResponse
        }
        return try JSONPParser.parse(data)
    }

    private func stringValue(_ value: Any?) -> String {
        if let value = value as? String { return value }
        if let value = value as? NSNumber { return value.stringValue }
        return ""
    }

    private func nonemptyString(_ value: Any?) -> String? {
        let text = stringValue(value)
        return text.isEmpty ? nil : text
    }
}
