import { describe, expect, it } from "vitest";
import { detectSecretKind, maskSecret } from "./secrets";

describe("detectSecretKind", () => {
  it("identifies an OpenAI sk- token as 'token'", () => {
    expect(detectSecretKind("sk-abcdefghijklmnopqrstuvwxyz0123456789")).toBe(
      "token",
    );
  });

  it("identifies a GitHub PAT (ghp_) as 'token'", () => {
    expect(
      detectSecretKind("ghp_AAAA1111BBBB2222CCCC3333DDDD4444EEEE"),
    ).toBe("token");
  });

  it("identifies an AWS access key id (AKIA...) as 'token'", () => {
    expect(detectSecretKind("AKIAABCDEFGHIJKLMNOP")).toBe("token");
  });

  it("identifies a JWT bearer string as 'token'", () => {
    const jwt =
      "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkphbmUifQ.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";
    expect(detectSecretKind(jwt)).toBe("token");
  });

  it("identifies a private key block as 'token'", () => {
    expect(
      detectSecretKind("-----BEGIN RSA PRIVATE KEY-----\nMIIBOgIB..."),
    ).toBe("token");
  });

  it("identifies a generic password= value as 'password'", () => {
    expect(detectSecretKind("password=hunter2hunter2")).toBe("password");
  });

  it("returns null for plain prose", () => {
    expect(detectSecretKind("just a normal sentence with no secrets")).toBeNull();
  });

  it("returns null for empty string", () => {
    expect(detectSecretKind("")).toBeNull();
  });
});

describe("maskSecret", () => {
  it("keeps the last 4 chars visible by default", () => {
    expect(maskSecret("supersecretvalue1234")).toBe("••••••••••••1234");
  });

  it("returns an empty string for empty input", () => {
    expect(maskSecret("")).toBe("");
  });

  it("masks fully when value is shorter than the tail", () => {
    expect(maskSecret("ab", 4)).toBe("••");
  });
});
