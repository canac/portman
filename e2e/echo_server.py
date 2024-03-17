import http.server
import os
import socketserver
from http import HTTPStatus

class Handler(http.server.SimpleHTTPRequestHandler):
    def do_GET(self):
        self.send_response(HTTPStatus.OK)
        self.end_headers()
        self.wfile.write(bytes(os.environ['MESSAGE'], 'utf-8'))

httpd = socketserver.TCPServer(('', int(os.environ['PORT'])), Handler)
httpd.serve_forever()
