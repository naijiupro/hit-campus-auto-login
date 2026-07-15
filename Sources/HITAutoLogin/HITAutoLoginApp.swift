import AppKit
import SwiftUI

@main
struct HITAutoLoginApp: App {
    @StateObject private var model = AppModel()

    var body: some Scene {
        MenuBarExtra {
            MenuContentView(model: model)
        } label: {
            Label("HIT 校园网", systemImage: model.statusIconName)
        }
        .menuBarExtraStyle(.window)
    }
}

private struct MenuContentView: View {
    @ObservedObject var model: AppModel
    @State private var showPassword = false

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack {
                Image(systemName: "building.columns.fill")
                    .font(.title2)
                    .foregroundStyle(.blue)
                VStack(alignment: .leading, spacing: 2) {
                    Text("HIT 校园网自动登录")
                        .font(.headline)
                    Text("HIT-WLAN · wp.hit.edu.cn")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }

            VStack(alignment: .leading, spacing: 8) {
                TextField("学号", text: $model.username)
                    .textFieldStyle(.roundedBorder)

                HStack(spacing: 6) {
                    Group {
                        if showPassword {
                            TextField("密码", text: $model.password)
                        } else {
                            SecureField("密码", text: $model.password)
                        }
                    }
                    .textFieldStyle(.roundedBorder)

                    Button {
                        showPassword.toggle()
                    } label: {
                        Image(systemName: showPassword ? "eye.slash" : "eye")
                    }
                    .buttonStyle(.borderless)
                    .help(showPassword ? "隐藏密码" : "显示密码")
                }

                Text("账号密码以普通偏好设置保存，不使用钥匙串或加密。")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }

            Toggle("登录 Mac 时自动启动", isOn: $model.launchAtLogin)
                .onChange(of: model.launchAtLogin) { _ in
                    model.launchAtLoginChanged()
                }

            HStack(spacing: 8) {
                Button("保存") {
                    model.saveSettings()
                }
                Button("保存并立即检测") {
                    model.runNow()
                }
                .keyboardShortcut(.defaultAction)
                .disabled(model.isRunning)
            }

            Divider()

            HStack(alignment: .top, spacing: 9) {
                if model.isRunning {
                    ProgressView()
                        .controlSize(.small)
                        .padding(.top, 2)
                } else {
                    Image(systemName: model.statusIconName)
                        .foregroundStyle(statusColor)
                        .padding(.top, 1)
                }

                VStack(alignment: .leading, spacing: 3) {
                    Text(model.statusMessage)
                        .font(.subheadline.weight(.medium))
                    Text(model.statusDetail)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .textSelection(.enabled)
                }
            }

            HStack {
                Text("事件触发运行，无后台循环轮询")
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
                Spacer()
                Button("退出") {
                    NSApplication.shared.terminate(nil)
                }
                .buttonStyle(.borderless)
            }
        }
        .padding(16)
        .frame(width: 350)
    }

    private var statusColor: Color {
        switch model.statusLevel {
        case .idle: return .secondary
        case .working: return .blue
        case .success: return .green
        case .warning: return .orange
        case .failure: return .red
        }
    }
}
