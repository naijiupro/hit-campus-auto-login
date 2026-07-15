import AppKit
import Combine
import Foundation

enum StatusLevel {
    case idle
    case working
    case success
    case warning
    case failure
}

enum RunTrigger: String {
    case launch = "登录启动"
    case wake = "系统唤醒"
    case screenWake = "屏幕唤醒"
    case sessionActive = "会话恢复"
    case manual = "手动检测"

    var isAutomatic: Bool { self != .manual }
}

@MainActor
final class AppModel: ObservableObject {
    @Published var username: String
    @Published var password: String
    @Published var launchAtLogin: Bool
    @Published private(set) var statusLevel: StatusLevel = .idle
    @Published private(set) var statusMessage = "等待首次检测"
    @Published private(set) var statusDetail = "仅在登录、系统唤醒或屏幕唤醒时运行一次"
    @Published private(set) var isRunning = false

    private let defaults = UserDefaults.standard
    private let workflow = NetworkWorkflow()
    private var observerTokens: [NSObjectProtocol] = []
    private var lastAutomaticRun: Date?

    private enum Keys {
        static let username = "campusUsername"
        static let password = "campusPassword"
        static let launchAtLogin = "launchAtLogin"
    }

    init() {
        username = defaults.string(forKey: Keys.username) ?? ""
        password = defaults.string(forKey: Keys.password) ?? ""
        launchAtLogin = defaults.object(forKey: Keys.launchAtLogin) as? Bool ?? true

        observeWorkspaceEvents()
        Task { [weak self] in
            guard let self else { return }
            self.refreshLaunchAgent()
            try? await Task.sleep(nanoseconds: 2_000_000_000)
            self.run(trigger: .launch)
        }
    }

    deinit {
        for token in observerTokens {
            NSWorkspace.shared.notificationCenter.removeObserver(token)
        }
    }

    var statusIconName: String {
        switch statusLevel {
        case .idle: return "wifi"
        case .working: return "arrow.triangle.2.circlepath"
        case .success: return "wifi.circle.fill"
        case .warning: return "exclamationmark.triangle.fill"
        case .failure: return "wifi.exclamationmark"
        }
    }

    func saveSettings(showConfirmation: Bool = true) {
        username = username.trimmingCharacters(in: .whitespacesAndNewlines)
        defaults.set(username, forKey: Keys.username)
        defaults.set(password, forKey: Keys.password)
        defaults.set(launchAtLogin, forKey: Keys.launchAtLogin)
        refreshLaunchAgent()

        if showConfirmation, statusLevel != .failure {
            statusLevel = .idle
            statusMessage = "设置已保存"
            statusDetail = launchAtLogin ? "下次登录 Mac 时会自动启动" : "登录时自动启动已关闭"
        }
    }

    func launchAtLoginChanged() {
        defaults.set(launchAtLogin, forKey: Keys.launchAtLogin)
        refreshLaunchAgent()
    }

    func runNow() {
        saveSettings(showConfirmation: false)
        run(trigger: .manual)
    }

    func run(trigger: RunTrigger) {
        guard !isRunning else { return }

        if trigger.isAutomatic,
           let lastAutomaticRun,
           Date().timeIntervalSince(lastAutomaticRun) < 20 {
            return
        }
        if trigger.isAutomatic {
            lastAutomaticRun = Date()
        }

        let cleanUsername = username.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !cleanUsername.isEmpty, !password.isEmpty else {
            statusLevel = .warning
            statusMessage = "请先填写学号和密码"
            statusDetail = "点击菜单栏图标完成设置"
            return
        }

        isRunning = true
        statusLevel = .working
        statusMessage = "开始检测"
        statusDetail = "触发原因：\(trigger.rawValue)"
        let configuration = AutoLoginConfiguration(
            username: cleanUsername,
            password: password,
            ssid: "HIT-WLAN"
        )

        Task { [weak self] in
            guard let self else { return }
            do {
                let result = try await self.workflow.execute(configuration: configuration) { message in
                    self.statusLevel = .working
                    self.statusMessage = message
                }
                self.isRunning = false
                self.statusLevel = .success
                self.statusMessage = result.message
                self.statusDetail = "\(trigger.rawValue) · \(Self.timeFormatter.string(from: Date()))"
            } catch {
                self.isRunning = false
                self.statusLevel = .failure
                self.statusMessage = error.localizedDescription
                self.statusDetail = "\(trigger.rawValue) · \(Self.timeFormatter.string(from: Date()))"
            }
        }
    }

    private func observeWorkspaceEvents() {
        let center = NSWorkspace.shared.notificationCenter
        let events: [(Notification.Name, RunTrigger)] = [
            (NSWorkspace.didWakeNotification, .wake),
            (NSWorkspace.screensDidWakeNotification, .screenWake),
            (NSWorkspace.sessionDidBecomeActiveNotification, .sessionActive)
        ]

        for (name, trigger) in events {
            let token = center.addObserver(forName: name, object: nil, queue: .main) { [weak self] _ in
                Task { @MainActor in
                    self?.run(trigger: trigger)
                }
            }
            observerTokens.append(token)
        }
    }

    private func refreshLaunchAgent() {
        do {
            try LaunchAgentManager.setEnabled(launchAtLogin)
        } catch {
            statusLevel = .failure
            statusMessage = "无法更新登录启动项"
            statusDetail = error.localizedDescription
        }
    }

    private static let timeFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "zh_CN")
        formatter.dateFormat = "MM-dd HH:mm:ss"
        return formatter
    }()
}

enum LaunchAgentManager {
    private static let label = "cn.edu.hit.HITAutoLogin"

    static func setEnabled(_ enabled: Bool) throws {
        let fileManager = FileManager.default
        let launchAgents = fileManager.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/LaunchAgents", isDirectory: true)
        let plistURL = launchAgents.appendingPathComponent("\(label).plist")

        if !enabled {
            if fileManager.fileExists(atPath: plistURL.path) {
                try fileManager.removeItem(at: plistURL)
            }
            return
        }

        guard let executableURL = Bundle.main.executableURL else {
            throw AutoLoginError.commandFailed("无法确定应用程序路径")
        }
        try fileManager.createDirectory(
            at: launchAgents,
            withIntermediateDirectories: true
        )

        let propertyList: [String: Any] = [
            "Label": label,
            "ProgramArguments": [executableURL.path],
            "RunAtLoad": true,
            "KeepAlive": false,
            "LimitLoadToSessionType": "Aqua"
        ]
        let data = try PropertyListSerialization.data(
            fromPropertyList: propertyList,
            format: .xml,
            options: 0
        )
        try data.write(to: plistURL, options: .atomic)
    }
}
