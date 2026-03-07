# Swagger Restoration Files

## 1. public/openapi.yaml
```yaml
openapi: 3.0.0
info:
  title: Brilliant Little Gnome API
  version: 1.0.0
  description: Robust API documentation for the brilliant little gnome project.
servers:
  - url: /api/v1
paths:
  /auth/cookies:
    post:
      summary: Set authentication cookies
      responses:
        '200':
          description: OK
  /courses:
    get:
      summary: List all courses
      responses:
        '200':
          description: OK
  /courses/{id}/summary:
    get:
      summary: Get course summary
      parameters:
        - name: id
          in: path
          required: true
          schema:
            type: integer
      responses:
        '200':
          description: OK
  /dashboard/summary:
    get:
      summary: Get dashboard summary
      responses:
        '200':
          description: OK
```

## 2. views/docs.erb
```html
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <title>API Documentation</title>
    <link rel="stylesheet" href="https://unpkg.com/swagger-ui-dist@4.5.0/swagger-ui.css" />
</head>
<body>
    <div id="swagger-ui"></div>
    <script src="https://unpkg.com/swagger-ui-dist@4.5.0/swagger-ui-bundle.js"></script>
    <script>
        window.onload = () => {
            window.ui = SwaggerUIBundle({
                url: '/openapi.yaml',
                dom_id: '#swagger-ui',
            });
        };
    </script>
</body>
</html>
```

## 3. app.rb Change
Add this line:
```ruby
get('/docs') { erb :docs }
```
