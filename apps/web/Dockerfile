FROM node:24.20.0-alpine

RUN npm install --global npm@12.0.2 pnpm@11.25.0

WORKDIR /app

RUN mkdir -p /app/node_modules /pnpm/store \
    && chown -R node:node /app /pnpm

USER node

CMD ["pnpm", "dev", "--hostname", "0.0.0.0"]
