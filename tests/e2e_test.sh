#!/bin/bash
set -e

if [ -z "$1" ]; then
    echo "Usage: ./tests/e2e_test.sh <RESPONDER_IP>"
    exit 1
fi

RESPONDER_IP=$1
RESPONDER_PORT=8124
URL="http://$RESPONDER_IP:$RESPONDER_PORT"

echo "================================================="
echo "   BACnet-MQTT Bridge E2E Test Suite             "
echo "   Target Responder: $RESPONDER_IP               "
echo "================================================="

echo "[1/4] Starting Local Mosquitto Broker (Docker)..."
# If mosquitto is running on host, this might fail, which is okay if it's already bound
docker run -d --name mqtt-test -p 1883:1883 eclipse-mosquitto 2>/dev/null || echo "Broker might already be running locally. Proceeding..."

echo "Building bacnet-mqtt-gateway..."
cargo build -p bacnet-mqtt-gateway --quiet

echo "[2/4] Starting Local Gateway..."
RUST_LOG=info target/debug/bacnet-mqtt-gateway > tests/gateway.log 2>&1 &
GATEWAY_PID=$!

function cleanup {
    echo "Cleaning up gateway (PID $GATEWAY_PID)..."
    kill $GATEWAY_PID 2>/dev/null || true
}
trap cleanup EXIT

sleep 3

echo "[3/4] Triggering Who-Is/I-Am exchange..."
echo "Forcing responder to broadcast I-Am..."
curl -s -X POST $URL/iam > /dev/null

echo "Waiting for MQTT Auto-Discovery payload..."
DISCOVERY=$(timeout 5 mosquitto_sub -t "homeassistant/sensor/#" -C 1) || true
if [ -z "$DISCOVERY" ]; then
    echo "❌ FAILED: No discovery payload received on MQTT."
    exit 1
else
    echo "✅ SUCCESS: Received Discovery Payload."
fi

echo "[4/4] Testing Data Polling and Latency..."
TEST_VAL="88.5"
echo "Setting Remote Responder AnalogInput to $TEST_VAL..."
curl -s -X POST $URL/value/$TEST_VAL > /dev/null

echo "Waiting up to 10 seconds for Gateway to poll and publish to MQTT..."
START_MS=$(date +%s%3N)
# The gateway publishes to homeassistant/sensor/bacnet_<ip>/state
# We will use mosquitto_sub to grab exactly one message from that wildcard topic matching the expected value.

# We loop because mosquitto_sub might grab an old retained message or a previous poll result.
FOUND=false
while true; do
    STATE=$(timeout 2 mosquitto_sub -t "homeassistant/sensor/+/state" -C 1) || true
    if [ "$STATE" == "$TEST_VAL" ]; then
        END_MS=$(date +%s%3N)
        LATENCY=$((END_MS - START_MS))
        echo "✅ SUCCESS: Value $TEST_VAL received over MQTT in ${LATENCY}ms!"
        FOUND=true
        break
    fi
    NOW=$(date +%s%3N)
    if [ $((NOW - START_MS)) -gt 10000 ]; then
        break
    fi
done

if [ "$FOUND" = false ]; then
    echo "❌ FAILED: Timeout waiting for state update to $TEST_VAL."
    echo "Check tests/gateway.log for more details."
    exit 1
fi

echo ""
echo "================================================="
echo "   All Tests Passed Successfully!                "
echo "================================================="
exit 0
