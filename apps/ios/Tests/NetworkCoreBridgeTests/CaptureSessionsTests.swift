import Foundation
@testable import NetworkCoreBridge
import XCTest

final class CaptureSessionsTests: XCTestCase {
    private func request(_ command: RuntimeControl) -> String {
        String(decoding: command.request, as: UTF8.self)
    }

    func testExportRequestCommands() {
        XCTAssertEqual(
            request(.exportRequest(.curl, id: 4, exchange: 1, redact: true)),
            #"{"cmd":"export_capture","exchange":1,"format":"curl","id":4,"redact":true}"#
        )
        XCTAssertEqual(
            request(.exportRequest(.code(.kotlin), id: 4, exchange: 0, redact: false)),
            #"{"cmd":"export_capture","exchange":0,"format":"code","id":4,"language":"kotlin","redact":false}"#
        )
        XCTAssertEqual(PacketCapture.RequestExport.all.count, 4 + PacketCapture.CodeLanguage.allCases.count)
        XCTAssertEqual(Set(PacketCapture.RequestExport.all.map(\.id)).count, PacketCapture.RequestExport.all.count)
    }

    func testSessionCommands() {
        XCTAssertEqual(
            request(.setCaptureStorage(dir: "x", keyBase64: "AA==", retentionDays: 7)),
            #"{"cmd":"set_capture_storage","dir":"x","key_base64":"AA==","retention_days":7}"#
        )
        XCTAssertEqual(request(.saveCapture(name: nil)), #"{"cmd":"save_capture"}"#)
        XCTAssertEqual(request(.saveCapture(name: "登录")), #"{"cmd":"save_capture","name":"登录"}"#)
        XCTAssertEqual(request(.saveCapture(name: "x", run: true)), #"{"cmd":"save_capture","name":"x","run":true}"#)
        XCTAssertEqual(
            request(.copyCapture(session: nil, keyBase64: "AA==", name: "自动")),
            #"{"cmd":"copy_capture","key_base64":"AA==","name":"自动"}"#
        )
        XCTAssertEqual(
            request(.copyCapture(session: "1-0a0b0c0d", keyBase64: "AA==", name: nil)),
            #"{"cmd":"copy_capture","key_base64":"AA==","session":"1-0a0b0c0d"}"#
        )
        XCTAssertEqual(
            request(.importCapture(session: "1-0a0b0c0d", dataBase64: "QQ==")),
            #"{"cmd":"import_capture","data_base64":"QQ==","session":"1-0a0b0c0d"}"#
        )
        XCTAssertEqual(request(.captureSessions), #"{"cmd":"capture_sessions"}"#)
        XCTAssertEqual(request(.loadCapture("s1")), #"{"cmd":"load_capture","session":"s1"}"#)
        XCTAssertEqual(request(.deleteCapture("s1")), #"{"cmd":"delete_capture","session":"s1"}"#)
    }

    func testDecodesSessions() throws {
        let answer = try XCTUnwrap(RuntimeControl.decode(PacketCapture.SessionsAnswer.self, from: Data("""
        {"sessions":[
          {"session":"a","name":"登录","saved_at_ms":1700000000000,"connections":3,"requests":9,"size":2048,
           "expires_at_ms":1700604800000},
          {"session":"b","saved_at_ms":1700000001000,"unreadable":true,"size":10,"expires_at_ms":1700604801000}
        ]}
        """.utf8)))
        XCTAssertEqual(answer.sessions.map(\.id), ["a", "b"])
        XCTAssertEqual(answer.sessions[0].name, "登录")
        XCTAssertEqual(answer.sessions[0].requests, 9)
        XCTAssertFalse(answer.sessions[0].unreadable)
        XCTAssertNil(answer.sessions[1].name)
        XCTAssertNil(answer.sessions[1].requests)
        XCTAssertTrue(answer.sessions[1].unreadable)

        let saved = try XCTUnwrap(RuntimeControl.decode(PacketCapture.SavedAnswer.self, from: Data("""
        {"session":{"session":"c","name":null,"saved_at_ms":1700000000000,"connections":1,"requests":2}}
        """.utf8)))
        XCTAssertEqual(saved.session.id, "c")
        XCTAssertEqual(saved.session.size, 0)
    }
}
