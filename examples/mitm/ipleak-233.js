// MITM test script: every IPv4 address in ipleak.net responses becomes 233.333.333.333.
// 233.333.333.333 is not a valid address, so a rewritten page is easy to recognise.
const FAKE_IP = "233.333.333.333";
const IPV4 = /\b(?:\d{1,3}\.){3}\d{1,3}\b/g;

const body = $response.body;
if (typeof body === "string" && body.length > 0) {
  $done({ body: body.replace(IPV4, FAKE_IP) });
} else {
  $done({});
}
