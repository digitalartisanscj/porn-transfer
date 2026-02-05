const express = require('express');
const path = require('path');
const fs = require('fs');
const { Bonjour } = require('bonjour-service');

const app = express();
const PORT = 8080;
const UPDATES_DIR = path.join(__dirname, 'updates');

// Ensure updates directory exists
if (!fs.existsSync(UPDATES_DIR)) {
    fs.mkdirSync(UPDATES_DIR, { recursive: true });
}

// CORS for local network
app.use((req, res, next) => {
    res.header('Access-Control-Allow-Origin', '*');
    res.header('Access-Control-Allow-Headers', 'Origin, X-Requested-With, Content-Type, Accept');
    next();
});

// Logging middleware
app.use((req, res, next) => {
    const timestamp = new Date().toLocaleTimeString();
    console.log(`[${timestamp}] ${req.method} ${req.url} from ${req.ip}`);
    next();
});

// Serve static files from updates directory
app.use('/receiver', express.static(path.join(UPDATES_DIR, 'receiver')));
app.use('/sender', express.static(path.join(UPDATES_DIR, 'sender')));

// Health check endpoint
app.get('/health', (req, res) => {
    res.json({ status: 'ok', timestamp: new Date().toISOString() });
});

// List available updates
app.get('/list', (req, res) => {
    const result = { receiver: null, sender: null };

    const receiverJson = path.join(UPDATES_DIR, 'receiver', 'latest.json');
    if (fs.existsSync(receiverJson)) {
        try {
            result.receiver = JSON.parse(fs.readFileSync(receiverJson, 'utf8'));
        } catch (e) {
            result.receiver = { error: 'Invalid JSON' };
        }
    }

    const senderJson = path.join(UPDATES_DIR, 'sender', 'latest.json');
    if (fs.existsSync(senderJson)) {
        try {
            result.sender = JSON.parse(fs.readFileSync(senderJson, 'utf8'));
        } catch (e) {
            result.sender = { error: 'Invalid JSON' };
        }
    }

    res.json(result);
});

// Start server
app.listen(PORT, '0.0.0.0', () => {
    console.log('');
    console.log('╔════════════════════════════════════════════════════════════╗');
    console.log('║         PORN TRANSFER UPDATE SERVER                        ║');
    console.log('╠════════════════════════════════════════════════════════════╣');
    console.log(`║  Server running on port ${PORT}                               ║`);
    console.log('║                                                            ║');
    console.log('║  Endpoints:                                                ║');
    console.log('║    /receiver/latest.json  - Receiver update manifest       ║');
    console.log('║    /sender/latest.json    - Sender update manifest         ║');
    console.log('║    /list                  - List all available updates     ║');
    console.log('║    /health                - Health check                   ║');
    console.log('║                                                            ║');
    console.log('╚════════════════════════════════════════════════════════════╝');
    console.log('');

    // Advertise via mDNS/Bonjour
    const bonjour = new Bonjour();
    bonjour.publish({
        name: 'porn-transfer-updates',
        type: 'http',
        port: PORT,
        txt: {
            path: '/',
            version: '1.0'
        }
    });

    console.log('📡 mDNS service advertised as "porn-transfer-updates"');
    console.log('');
    console.log('Waiting for update requests...');
    console.log('');
});
